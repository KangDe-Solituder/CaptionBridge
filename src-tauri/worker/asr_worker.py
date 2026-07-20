"""LiveCaption local ASR worker.

Protocol: one JSON object per stdin/stdout line. Diagnostics go to stderr so
stdout remains machine-readable. Audio is captured from the current Windows
default speaker through WASAPI loopback and normalized to 16 kHz mono float32.
"""
from __future__ import annotations

import json
import queue
import sys
import threading
import time
import traceback
from pathlib import Path

import numpy as np

# NDJSON must be independent of the Windows active code page.  Escaping
# non-ASCII text keeps every protocol line ASCII-only even on GBK systems.
if hasattr(sys.stdin, "reconfigure"):
    sys.stdin.reconfigure(encoding="utf-8")
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="strict", newline="\n", line_buffering=True)

def emit(kind: str, **payload) -> None:
    print(json.dumps({"type": kind, **payload}, ensure_ascii=True), flush=True)


class Worker:
    def __init__(self) -> None:
        self.model = None
        self.model_id = None
        self.profile = {
            "threshold": 0.5,
            "silence_commit_ms": 500,
            "max_segment_ms": 8000,
            "partial_interval_ms": 800,
        }
        self.stop_capture = threading.Event()
        self.capture_thread: threading.Thread | None = None
        self.inference_lock = threading.Lock()
        self.utterance: list[np.ndarray] = []
        self.utterance_started = 0.0
        self.last_voice = 0.0
        self.last_partial = 0.0
        self.stream_started = 0.0

    def load(self, command: dict) -> None:
        from faster_whisper import WhisperModel
        from faster_whisper.vad import get_vad_model

        path = Path(command["model_path"])
        if not path.joinpath("model.bin").is_file():
            raise FileNotFoundError(f"model.bin not found in {path}")
        emit("loading_progress", progress=0.05, message="initializing_cuda")
        self.model = WhisperModel(
            str(path),
            device=command.get("device", "cuda"),
            compute_type=command.get("compute_type", "int8_float16"),
            local_files_only=True,
        )
        emit("loading_progress", progress=0.85, message="initializing_silero_vad")
        get_vad_model()
        self.model_id = command.get("model_id")
        self.profile.update(command.get("vad", {}))
        emit("ready", model_id=self.model_id, device=command.get("device", "cuda"))

    def dry_run(self) -> None:
        if self.model is None:
            raise RuntimeError("model_not_loaded")
        silent = np.zeros(16000, dtype=np.float32)
        started = time.perf_counter()
        list(self.model.transcribe(
            silent, language="ja", task="transcribe", beam_size=1,
            condition_on_previous_text=False, vad_filter=True,
            vad_parameters={"threshold": self.profile["threshold"]},
        )[0])
        emit("health", healthy=True, latency_ms=int((time.perf_counter() - started) * 1000), detail="dry_run_ok")

    def start(self) -> None:
        if self.model is None:
            raise RuntimeError("model_not_loaded")
        if self.capture_thread and self.capture_thread.is_alive():
            return
        self.stop_capture.clear()
        self.capture_thread = threading.Thread(target=self.capture_loop_safe, name="wasapi-loopback", daemon=True)
        self.capture_thread.start()

    def stop(self, flush: bool = True) -> None:
        self.stop_capture.set()
        if self.capture_thread and self.capture_thread.is_alive():
            self.capture_thread.join(timeout=2)
        if flush:
            self.commit(final=True)
        emit("health", healthy=True, detail="capture_stopped")

    def unload(self) -> None:
        self.stop(flush=False)
        self.model = None
        self.model_id = None
        try:
            import ctranslate2
            ctranslate2.set_random_seed(0)
        except Exception:
            pass
        emit("unloaded")

    def capture_loop_safe(self) -> None:
        while not self.stop_capture.is_set():
            try:
                self.capture_loop()
            except Exception as error:
                traceback.print_exc(file=sys.stderr)
                emit("error", code="audio_capture_failed", message=str(error), recoverable=False)
                return

    def capture_loop(self) -> None:
        import soundcard as sc
        from faster_whisper.vad import get_vad_model

        vad = get_vad_model()
        speaker = sc.default_speaker()
        if speaker is None:
            raise RuntimeError("default_output_device_not_found")
        loopback = sc.get_microphone(id=str(speaker.id), include_loopback=True)
        if loopback is None:
            raise RuntimeError("default_output_loopback_not_found")
        self.stream_started = time.perf_counter()
        # SoundCard performs shared-mode WASAPI conversion to the requested rate.
        with loopback.recorder(samplerate=16000, channels=1, blocksize=1600) as recorder:
            emit("health", healthy=True, detail="capture_started")
            frame_index = 0
            while not self.stop_capture.is_set():
                frame = recorder.record(numframes=1600).reshape(-1).astype(np.float32, copy=False)
                frame_index += 1
                if frame_index % 10 == 0:
                    current = sc.default_speaker()
                    if current is None or str(current.id) != str(speaker.id):
                        emit("health", healthy=False, detail="default_output_changed_reopening")
                        return
                now = time.perf_counter()
                padded = np.pad(frame, (0, (-len(frame)) % 512))
                probabilities = vad(padded.reshape(1, -1))[0]
                probability = float(np.max(probabilities)) if len(probabilities) else 0.0
                voiced = probability >= float(self.profile["threshold"])
                if voiced:
                    if not self.utterance:
                        self.utterance_started = now
                    self.last_voice = now
                    self.utterance.append(frame)
                elif self.utterance:
                    self.utterance.append(frame)

                if self.utterance and (now - self.last_partial) * 1000 >= int(self.profile["partial_interval_ms"]):
                    self.last_partial = now
                    self.transcribe_async(final=False)
                silence_ms = (now - self.last_voice) * 1000 if self.last_voice else 0
                length_ms = (now - self.utterance_started) * 1000 if self.utterance_started else 0
                if self.utterance and (silence_ms >= int(self.profile["silence_commit_ms"]) or length_ms >= int(self.profile["max_segment_ms"])):
                    self.commit(final=True)

    def snapshot(self) -> tuple[np.ndarray, int, int]:
        audio = np.concatenate(self.utterance) if self.utterance else np.empty(0, dtype=np.float32)
        start_ms = int((self.utterance_started - self.stream_started) * 1000)
        end_ms = start_ms + int(len(audio) / 16)
        return audio, max(0, start_ms), max(0, end_ms)

    def transcribe_async(self, final: bool) -> None:
        audio, start_ms, end_ms = self.snapshot()
        if len(audio) < 3200 or not self.inference_lock.acquire(blocking=False):
            return
        threading.Thread(target=self.transcribe, args=(audio, start_ms, end_ms, final), daemon=True).start()

    def commit(self, final: bool = True) -> None:
        audio, start_ms, end_ms = self.snapshot()
        self.utterance = []
        self.utterance_started = 0.0
        self.last_voice = 0.0
        if len(audio) < 3200:
            return
        self.inference_lock.acquire()
        threading.Thread(target=self.transcribe, args=(audio, start_ms, end_ms, final), daemon=True).start()

    def transcribe(self, audio: np.ndarray, start_ms: int, end_ms: int, final: bool) -> None:
        try:
            started = time.perf_counter()
            segments, _ = self.model.transcribe(
                audio,
                language="ja",
                task="transcribe",
                beam_size=3 if final else 1,
                best_of=1,
                condition_on_previous_text=False,
                vad_filter=final,
                vad_parameters={
                    "threshold": self.profile["threshold"],
                    "min_silence_duration_ms": self.profile["silence_commit_ms"],
                    "max_speech_duration_s": self.profile["max_segment_ms"] / 1000,
                    "speech_pad_ms": 150 if self.profile["threshold"] >= 0.5 else 300,
                },
            )
            text = "".join(segment.text for segment in segments).strip()
            if text:
                emit("final" if final else "partial", text=text, start_ms=start_ms, end_ms=end_ms,
                     latency_ms=int((time.perf_counter() - started) * 1000), model_id=self.model_id)
        except Exception as error:
            emit("error", code="inference_failed", message=str(error), recoverable=True)
        finally:
            self.inference_lock.release()


def main() -> None:
    worker = Worker()
    for line in sys.stdin:
        try:
            command = json.loads(line)
            name = command.get("command")
            if name == "load": worker.load(command)
            elif name == "dry_run": worker.dry_run()
            elif name == "start": worker.start()
            elif name == "stop": worker.stop()
            elif name == "flush": worker.commit()
            elif name == "unload": worker.unload()
            elif name == "shutdown":
                worker.stop(flush=False)
                emit("shutdown")
                return
            else: raise ValueError(f"unknown_command:{name}")
        except Exception as error:
            traceback.print_exc(file=sys.stderr)
            emit("error", code="worker_command_failed", message=str(error), recoverable=False)


if __name__ == "__main__":
    main()
