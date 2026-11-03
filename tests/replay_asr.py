"""Replay a 16 kHz PCM WAV through the production capture/inference queue.

Run with the ASR build venv. No speaker playback, microphone capture or LLM
requests; the WAV replaces WASAPI input. Outputs JSON evidence, not gold scores.
"""
import argparse
import json
from pathlib import Path
import sys
import time
import wave

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src-tauri" / "worker"))
import asr_worker

parser = argparse.ArgumentParser()
parser.add_argument("--audio", required=True)
parser.add_argument("--model", required=True)
parser.add_argument("--mode", choices=["default", "quick"], default="default")
parser.add_argument("--seconds", type=float, default=45)
parser.add_argument("--output", required=True)
args = parser.parse_args()

with wave.open(args.audio) as wav:
    assert wav.getframerate() == 16000 and wav.getsampwidth() == 2, "Expected 16kHz signed 16-bit PCM"
    audio = np.frombuffer(wav.readframes(int(args.seconds * 16000)), np.int16).reshape(-1, wav.getnchannels()).astype(np.float32) / 32768

worker = asr_worker.Worker()
events = []
started = time.perf_counter()
asr_worker.emit = lambda kind, **data: events.append(dict(type=kind, wall_seconds=time.perf_counter() - started, **data))
worker.load(dict(model_path=args.model, model_id=Path(args.model).name,
                 device="cuda", compute_type="int8_float16", vad={"mode": args.mode}))

load_seconds = time.perf_counter() - started
loading_events = events
events = []
max_queue = 0
def replay():
    global max_queue
    for i in range(0, len(audio), 1600):
        if worker.stop_capture.is_set():
            break
        # A captured frame is available only after its samples have arrived.
        due = started + min(i + 1600, len(audio)) / 16000
        worker.stop_capture.wait(max(0, due - time.perf_counter()))
        if worker.stop_capture.is_set():
            break
        for window in worker.windows.feed(audio[i:i + 1600]):
            worker.pending_windows.put(window)
        with worker.pending_windows.condition:
            max_queue = max(max_queue, len(worker.pending_windows.items))

def replay_safe():
    try:
        replay()
    except Exception as error:
        worker.stop_capture.set()
        asr_worker.emit("error", code="replay_failed", message=str(error), recoverable=False)


worker.capture_loop_safe = replay_safe
started = time.perf_counter()
worker.start()
worker.capture_thread.join()
worker.stop()
elapsed = time.perf_counter() - started
errors = [event for event in events if event["type"] == "error"]
finals = [event for event in events if event["type"] == "final"]
summary = dict(load_seconds=load_seconds, model=Path(args.model).name, mode=args.mode, audio_seconds=len(audio) / 16000,
               elapsed_seconds=elapsed, max_pending_windows=max_queue, final_count=len(finals),
               inference_p95_ms=float(np.percentile([e["latency_ms"] for e in finals], 95)) if finals else None,
               errors=errors, capture_stopped=events[-1].get("detail") == "capture_stopped")
Path(args.output).write_text(json.dumps(dict(summary=summary, loading_events=loading_events, events=events), ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(summary, ensure_ascii=True))
assert finals and not errors and summary["capture_stopped"], "Replay did not finish cleanly with final subtitles"
