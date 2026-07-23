"""LiveCaption local ASR worker.

Protocol: one JSON object per stdin/stdout line. Diagnostics go to stderr so
stdout remains machine-readable. Audio is captured from the current Windows
default speaker through WASAPI loopback and normalized to 16 kHz mono float32.
"""
from __future__ import annotations

import json
import ctypes
import os
import queue
import sys
import threading
import time
import traceback
from collections import deque
from dataclasses import dataclass
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


CHANNEL_WEIGHTS = {
    "left": (1.0, 0.0),
    "right": (0.0, 1.0),
    "mix": (0.5, 0.5),
}

CUDA_RUNTIME_FILES = ("cublas64_12.dll", "cublasLt64_12.dll")
CUDNN_RUNTIME_FILES = (
    "cudnn64_9.dll",
    "cudnn_adv64_9.dll",
    "cudnn_cnn64_9.dll",
    "cudnn_engines_precompiled64_9.dll",
    "cudnn_engines_runtime_compiled64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_ops64_9.dll",
)


@dataclass
class StereoChunk:
    frame: np.ndarray
    weights: tuple[float, float]


class StereoRouter:
    """Sticky ASMR voice-channel latch.

    Silence never releases a selected channel. A handoff needs sustained,
    positive speech evidence on another route and a rise above the noise
    baseline learned while the selected speaker was active.
    """

    PRESETS = {
        "stable": (8, 14, 0.24, 0.16),
        "standard": (6, 9, 0.18, 0.12),
        "responsive": (4, 5, 0.12, 0.08),
    }
    CENTER_MARGIN = 0.10

    def __init__(self, mode: str, threshold: float, sensitivity: str = "standard") -> None:
        self.mode = mode if mode in {"auto", "mix", "left", "right"} else "auto"
        self.threshold = max(threshold, 0.42) if self.mode == "auto" else threshold
        (
            self.initial_confirm_frames,
            self.switch_confirm_frames,
            self.switch_margin,
            self.noise_rise_margin,
        ) = self.PRESETS.get(sensitivity, self.PRESETS["standard"])
        self.smoothed = np.zeros(3, dtype=np.float32)
        self.noise_baseline = np.zeros(3, dtype=np.float32)
        self.noise_baseline_ready = np.zeros(3, dtype=np.bool_)
        self.selected: str | None = None
        self.candidate: str | None = None
        self.candidate_frames = 0

    @staticmethod
    def weights(route: str) -> tuple[float, float]:
        return CHANNEL_WEIGHTS.get(route, CHANNEL_WEIGHTS["mix"])

    @staticmethod
    def _route_score(route: str, scores: np.ndarray) -> float:
        return float(scores[{"left": 0, "right": 1, "mix": 2}[route]])

    def _desired_route(self) -> str:
        left, right, center = (float(value) for value in self.smoothed)
        if abs(left - right) <= self.CENTER_MARGIN and center >= max(left, right) - 0.06:
            return "mix"
        return "left" if left > right else "right"

    def reset(self) -> None:
        self.smoothed.fill(0.0)
        self.noise_baseline.fill(0.0)
        self.noise_baseline_ready.fill(False)
        self.selected = None
        self.candidate = None
        self.candidate_frames = 0

    @property
    def status(self) -> str:
        if self.mode != "auto":
            return f"asmr_locked_{self.mode}"
        if self.selected is None:
            return "asmr_searching"
        if self.candidate in {"left", "right"}:
            return f"asmr_switch_pending_{self.candidate}"
        return f"asmr_locked_{self.selected}"

    def _advance_candidate(self, desired: str, required_frames: int) -> bool:
        if self.candidate == desired:
            self.candidate_frames += 1
        else:
            self.candidate = desired
            self.candidate_frames = 1
        return self.candidate_frames >= required_frames

    def _cancel_candidate(self) -> None:
        self.candidate = None
        self.candidate_frames = 0

    def _learn_noise_baseline(self, scores: np.ndarray) -> None:
        if self.selected is None:
            return
        selected_index = {"left": 0, "right": 1, "mix": 2}[self.selected]
        if float(scores[selected_index]) < self.threshold:
            return
        for index in range(3):
            if index == selected_index:
                continue
            if self.noise_baseline_ready[index]:
                self.noise_baseline[index] += 0.08 * (
                    scores[index] - self.noise_baseline[index]
                )
            else:
                self.noise_baseline[index] = scores[index]
                self.noise_baseline_ready[index] = True

    def process(self, probabilities: np.ndarray) -> tuple[str, float, tuple[str, str, int] | None]:
        probabilities = np.asarray(probabilities, dtype=np.float32)
        rising = probabilities > self.smoothed
        alpha = np.where(rising, 0.48, 0.18)
        self.smoothed = self.smoothed + alpha * (probabilities - self.smoothed)

        if self.mode != "auto":
            route = self.mode
            return route, self._route_score(route, probabilities), None

        desired = self._desired_route()
        handoff = None
        if self.selected is None:
            desired_score = self._route_score(desired, self.smoothed)
            if desired_score >= self.threshold:
                if self._advance_candidate(desired, self.initial_confirm_frames):
                    self.selected = desired
                    handoff = ("searching", desired, self.candidate_frames)
                    self._cancel_candidate()
            else:
                self._cancel_candidate()
        elif desired != self.selected:
            current_score = self._route_score(self.selected, self.smoothed)
            desired_score = self._route_score(desired, self.smoothed)
            desired_index = {"left": 0, "right": 1, "mix": 2}[desired]
            noise_rise = (
                not self.noise_baseline_ready[desired_index]
                or desired_score - float(self.noise_baseline[desired_index])
                >= self.noise_rise_margin
            )
            should_switch = (
                desired_score >= self.threshold
                and current_score < self.threshold * 0.85
                and desired_score - current_score >= self.switch_margin
                and noise_rise
            )
            if should_switch:
                if self._advance_candidate(desired, self.switch_confirm_frames):
                    previous = self.selected
                    self.selected = desired
                    handoff = (previous, desired, self.candidate_frames)
                    self._cancel_candidate()
            else:
                self._cancel_candidate()
        else:
            self._cancel_candidate()

        self._learn_noise_baseline(self.smoothed)
        route = self.selected or desired
        probability = self._route_score(route, probabilities) if self.selected else 0.0
        return route, probability, handoff


class AcousticEvidenceTracker:
    """Reduces stationary ASMR noise before it reaches the routing state machine."""

    HISTORY_FRAMES = 20

    def __init__(self) -> None:
        self.log_rms = [deque(maxlen=self.HISTORY_FRAMES) for _ in range(3)]
        self.flatness = [deque(maxlen=self.HISTORY_FRAMES) for _ in range(3)]

    def reset(self) -> None:
        for history in (*self.log_rms, *self.flatness):
            history.clear()

    def process(self, candidates: np.ndarray, vad_probabilities: np.ndarray) -> np.ndarray:
        evidence = np.zeros(3, dtype=np.float32)
        window = np.hanning(candidates.shape[1]).astype(np.float32)
        for index, signal in enumerate(candidates):
            rms = float(np.sqrt(np.mean(np.square(signal), dtype=np.float32) + 1e-12))
            log_rms = 20.0 * np.log10(max(rms, 1e-6))
            spectrum = np.abs(np.fft.rfft(signal * window)) + 1e-8
            flatness = float(np.exp(np.mean(np.log(spectrum))) / np.mean(spectrum))
            self.log_rms[index].append(log_rms)
            self.flatness[index].append(flatness)

            if len(self.log_rms[index]) < 6:
                dynamics = 0.45
            else:
                values = np.asarray(self.log_rms[index], dtype=np.float32)
                dynamics = float(np.clip(
                    (np.percentile(values, 90) - np.percentile(values, 10)) / 12.0,
                    0.0,
                    1.0,
                ))
            average_flatness = float(np.mean(self.flatness[index]))
            flatness_penalty = float(np.clip((average_flatness - 0.18) / 0.55, 0.0, 1.0))
            speech_shape = float(np.clip(
                0.42 + 0.58 * dynamics - 0.18 * flatness_penalty,
                0.24,
                1.0,
            ))
            evidence[index] = float(vad_probabilities[index]) * speech_shape
        return evidence


class Worker:
    def __init__(self) -> None:
        self.model = None
        self.model_id = None
        self.profile = {
            "threshold": 0.5,
            "silence_commit_ms": 500,
            "max_segment_ms": 8000,
            "partial_interval_ms": 800,
            "channel_mode": "mono",
            "channel_switch_sensitivity": "standard",
            "suppress_non_speech_segments": False,
        }
        self.stop_capture = threading.Event()
        self.capture_thread: threading.Thread | None = None
        self.inference_lock = threading.Lock()
        self.utterance: list[np.ndarray | StereoChunk] = []
        self.utterance_started = 0.0
        self.last_voice = 0.0
        self.last_partial = 0.0
        self.stream_started = 0.0
        self.routing_reset = threading.Event()
        self.utterance_voice_frames = 0
        self.utterance_probability_sum = 0.0

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
        # Keep VAD disabled here: a silent VAD-filtered sample can return
        # before CTranslate2 enters the Whisper encoder, which previously let
        # incomplete cuDNN bundles pass diagnostics.
        silent = np.zeros(16000, dtype=np.float32)
        started = time.perf_counter()
        list(self.model.transcribe(
            silent, language="ja", task="transcribe", beam_size=1,
            best_of=1, condition_on_previous_text=False, vad_filter=False,
        )[0])
        emit(
            "health",
            healthy=True,
            latency_ms=int((time.perf_counter() - started) * 1000),
            detail="gpu_encoder_dry_run_ok",
        )

    def configure(self, command: dict) -> None:
        self.profile.update(command.get("vad", {}))
        self.routing_reset.clear()

    def probe_dependencies(self) -> None:
        import ctranslate2

        def load_runtime(name: str) -> tuple[bool, str | None]:
            if sys.platform != "win32":
                return False, "GPU worker currently supports Windows only"
            candidates = [Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))]
            candidates.extend(Path(entry) for entry in os.environ.get("PATH", "").split(os.pathsep) if entry)
            errors: list[str] = []
            for directory in candidates:
                library = directory / name
                if not library.is_file():
                    continue
                try:
                    with os.add_dll_directory(str(directory)):
                        ctypes.WinDLL(str(library))
                    return True, None
                except OSError as error:
                    errors.append(f"{library}: {error}")
            try:
                ctypes.WinDLL(name)
                return True, None
            except OSError as error:
                errors.append(str(error))
            return False, " | ".join(errors)

        def load_runtime_group(names: tuple[str, ...]) -> tuple[bool, str | None]:
            failures = []
            for name in names:
                loaded, error = load_runtime(name)
                if not loaded:
                    failures.append(f"{name}: {error or 'not found'}")
            return not failures, " | ".join(failures) if failures else None

        cuda_runtime_loaded, cuda_error = load_runtime_group(CUDA_RUNTIME_FILES)
        cudnn_runtime_loaded, cudnn_error = load_runtime_group(CUDNN_RUNTIME_FILES)
        device_count = ctranslate2.get_cuda_device_count()
        compute_types = sorted(ctranslate2.get_supported_compute_types("cuda")) if device_count else []
        emit(
            "dependency_probe",
            device_count=device_count,
            compute_types=compute_types,
            ctranslate2_version=ctranslate2.__version__,
            cuda_runtime_loaded=cuda_runtime_loaded,
            cudnn_runtime_loaded=cudnn_runtime_loaded,
            cuda_error=cuda_error,
            cudnn_error=cudnn_error,
        )

    def start(self) -> None:
        if self.model is None:
            raise RuntimeError("model_not_loaded")
        if self.capture_thread and self.capture_thread.is_alive():
            return
        self.stop_capture.clear()
        self.routing_reset.clear()
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
        self.utterance = []
        try:
            import ctranslate2
            ctranslate2.set_random_seed(0)
        except Exception:
            pass
        emit("unloaded")

    def reset_routing(self) -> None:
        if str(self.profile.get("channel_mode", "mono")) != "auto":
            raise RuntimeError("routing_reset_requires_auto_channel_mode")
        self.routing_reset.set()

    def capture_loop_safe(self) -> None:
        # SoundCard initializes COM when first imported, but subsequent
        # capture sessions run on new threads where COM must be initialized
        # again explicitly.
        import soundcard  # noqa: F401

        com_initialized = False
        if sys.platform == "win32":
            ole32 = ctypes.windll.ole32
            ole32.CoInitializeEx.restype = ctypes.c_long
            result = ole32.CoInitializeEx(None, 0)
            com_initialized = result in (0, 1)  # S_OK or S_FALSE
        try:
            while not self.stop_capture.is_set():
                try:
                    self.capture_loop()
                except Exception as error:
                    traceback.print_exc(file=sys.stderr)
                    emit("error", code="audio_capture_failed", message=str(error), recoverable=False)
                    return
        finally:
            if com_initialized:
                ctypes.windll.ole32.CoUninitialize()

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
        channel_mode = str(self.profile.get("channel_mode", "mono"))
        if channel_mode == "mono":
            self.capture_stream(sc, speaker, loopback, vad, channels=1, channel_mode="mono")
            return
        try:
            self.capture_stream(sc, speaker, loopback, vad, channels=2, channel_mode=channel_mode)
        except Exception as error:
            print(f"ASMR stereo capture unavailable, falling back to mono: {error}", file=sys.stderr, flush=True)
            emit("health", healthy=True, detail="asmr_stereo_unavailable_fallback_mono")
            self.capture_stream(sc, speaker, loopback, vad, channels=1, channel_mode="mono")

    def capture_stream(self, sc, speaker, loopback, vad, channels: int, channel_mode: str) -> None:
        router = (
            StereoRouter(
                channel_mode,
                float(self.profile["threshold"]),
                str(self.profile.get("channel_switch_sensitivity", "standard")),
            )
            if channels == 2
            else None
        )
        evidence_tracker = AcousticEvidenceTracker() if channels == 2 else None
        route_history = deque(maxlen=24)
        last_router_status: str | None = None
        # SoundCard performs shared-mode WASAPI conversion to 16 kHz.
        with loopback.recorder(samplerate=16000, channels=channels, blocksize=1600) as recorder:
            detail = "capture_started_mono" if channels == 1 else f"capture_started_stereo_{channel_mode}"
            emit("health", healthy=True, detail=detail)
            frame_index = 0
            while not self.stop_capture.is_set():
                captured = recorder.record(numframes=1600).astype(np.float32, copy=False)
                frame_index += 1
                if frame_index % 10 == 0:
                    current = sc.default_speaker()
                    if current is None or str(current.id) != str(speaker.id):
                        emit("health", healthy=False, detail="default_output_changed_reopening")
                        return

                now = time.perf_counter()
                if channels == 1:
                    frame = captured.reshape(-1)
                    padded = np.pad(frame, (0, (-len(frame)) % 512))
                    probabilities = vad(padded.reshape(1, -1))[0]
                    probability = float(np.max(probabilities)) if len(probabilities) else 0.0
                    self.accept_audio_frame(frame, probability, now)
                    continue

                stereo = captured.reshape(-1, 2)
                center = np.mean(stereo, axis=1, dtype=np.float32)
                raw_candidates = np.stack((stereo[:, 0], stereo[:, 1], center))
                padded_candidates = np.pad(
                    raw_candidates,
                    ((0, 0), (0, (-raw_candidates.shape[1]) % 512)),
                )
                vad_output = vad(padded_candidates)
                probabilities = (
                    np.max(vad_output.reshape(3, -1), axis=1)
                    if vad_output.size
                    else np.zeros(3)
                )
                routing_scores = (
                    evidence_tracker.process(raw_candidates, probabilities)
                    if channel_mode == "auto"
                    else probabilities
                )
                if self.routing_reset.is_set():
                    router.reset()
                    evidence_tracker.reset()
                    route_history.clear()
                    self.clear_utterance()
                    self.routing_reset.clear()
                    emit("health", healthy=True, detail="asmr_searching")

                route, probability, handoff = router.process(routing_scores)
                route_history.append((stereo.copy(), routing_scores.copy(), now))
                if router.status != last_router_status:
                    emit("health", healthy=True, detail=router.status)
                    last_router_status = router.status

                if handoff is not None:
                    previous, selected, frame_count = handoff
                    replayed = False
                    if previous == "searching" or not self.utterance:
                        self.clear_utterance()
                        replay_count = max(frame_count + 3, 10)
                        for historic_stereo, historic_scores, historic_now in list(route_history)[-replay_count:]:
                            selected_probability = StereoRouter._route_score(selected, historic_scores)
                            self.accept_audio_frame(
                                StereoChunk(historic_stereo, router.weights(selected)),
                                selected_probability,
                                historic_now,
                            )
                        replayed = True
                    else:
                        self.apply_channel_handoff(previous, selected, frame_count)
                    print(f"ASMR channel handoff: {previous} -> {selected}", file=sys.stderr, flush=True)
                    if replayed:
                        continue
                self.accept_audio_frame(
                    StereoChunk(stereo, router.weights(route)),
                    probability,
                    now,
                )

    def accept_audio_frame(
        self,
        frame: np.ndarray | StereoChunk,
        probability: float,
        now: float,
    ) -> None:
        voiced = probability >= float(self.profile["threshold"])
        if voiced:
            if not self.utterance:
                self.utterance_started = now
            self.last_voice = now
            self.utterance_voice_frames += 1
            self.utterance_probability_sum += probability
            self.utterance.append(frame)
        elif self.utterance:
            self.utterance.append(frame)

        if self.utterance and (now - self.last_partial) * 1000 >= int(self.profile["partial_interval_ms"]):
            self.last_partial = now
            self.transcribe_async(final=False)
        silence_ms = (now - self.last_voice) * 1000 if self.last_voice else 0
        length_ms = (now - self.utterance_started) * 1000 if self.utterance_started else 0
        if self.utterance and (
            silence_ms >= int(self.profile["silence_commit_ms"])
            or length_ms >= int(self.profile["max_segment_ms"])
        ):
            self.commit(final=True)

    def apply_channel_handoff(self, previous: str, selected: str, frame_count: int) -> None:
        stereo_chunks = [
            chunk for chunk in self.utterance[-frame_count:] if isinstance(chunk, StereoChunk)
        ]
        if not stereo_chunks:
            return
        previous_weights = np.asarray(StereoRouter.weights(previous), dtype=np.float32)
        selected_weights = np.asarray(StereoRouter.weights(selected), dtype=np.float32)
        for index, chunk in enumerate(stereo_chunks, start=1):
            alpha = index / (len(stereo_chunks) + 1)
            weights = previous_weights * (1.0 - alpha) + selected_weights * alpha
            chunk.weights = (float(weights[0]), float(weights[1]))

    def snapshot(self) -> tuple[np.ndarray, int, int]:
        chunks = []
        for chunk in self.utterance:
            if isinstance(chunk, StereoChunk):
                left, right = chunk.weights
                chunks.append(chunk.frame[:, 0] * left + chunk.frame[:, 1] * right)
            else:
                chunks.append(chunk)
        audio = np.concatenate(chunks) if chunks else np.empty(0, dtype=np.float32)
        start_ms = int((self.utterance_started - self.stream_started) * 1000)
        end_ms = start_ms + int(len(audio) / 16)
        return audio, max(0, start_ms), max(0, end_ms)

    def clear_utterance(self) -> None:
        self.utterance = []
        self.utterance_started = 0.0
        self.last_voice = 0.0
        self.utterance_voice_frames = 0
        self.utterance_probability_sum = 0.0

    def segment_has_speech(self, partial: bool = False) -> bool:
        if not bool(self.profile.get("suppress_non_speech_segments", False)):
            return True
        if str(self.profile.get("channel_mode", "mono")) != "auto":
            return True
        minimum_frames = 3 if partial else 4
        if self.utterance_voice_frames < minimum_frames:
            return False
        average_probability = self.utterance_probability_sum / max(1, self.utterance_voice_frames)
        return average_probability >= max(0.38, float(self.profile["threshold"]))

    def transcribe_async(self, final: bool) -> None:
        audio, start_ms, end_ms = self.snapshot()
        if (
            len(audio) < 3200
            or not self.segment_has_speech(partial=True)
            or not self.inference_lock.acquire(blocking=False)
        ):
            return
        threading.Thread(target=self.transcribe, args=(audio, start_ms, end_ms, final), daemon=True).start()

    def commit(self, final: bool = True) -> None:
        audio, start_ms, end_ms = self.snapshot()
        has_speech = self.segment_has_speech()
        self.clear_utterance()
        if len(audio) < 3200 or not has_speech:
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
            elif name == "configure": worker.configure(command)
            elif name == "dry_run": worker.dry_run()
            elif name == "probe_dependencies": worker.probe_dependencies()
            elif name == "start": worker.start()
            elif name == "stop": worker.stop()
            elif name == "flush": worker.commit()
            elif name == "reset_routing": worker.reset_routing()
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
