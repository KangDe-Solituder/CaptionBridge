"""Bounded audio windows and incremental Japanese ASR; independent of WASAPI.

Audio time, rather than inference completion time, drives all boundaries. Capture
never waits for inference. No language-model history is fed back into Whisper.
"""
from collections import deque
from dataclasses import dataclass
import threading
import time
import unicodedata

import numpy as np

RATE = 16000


@dataclass
class Window:
    epoch: int
    start: int
    end: int
    final: bool
    audio: np.ndarray


class Windows:
    def __init__(self, mode="default"):
        self.quick = mode == "quick"
        self.core = (4 if self.quick else 12) * RATE
        self.hop = (4 if self.quick else 2) * RATE
        self.overlap = 0 if self.quick else 4 * RATE
        self.epoch = self.end = self.origin = 0
        self.next = self.hop
        self.audio = np.empty(0, dtype=np.float32)

    def feed(self, frame):
        frame = np.asarray(frame, dtype=np.float32)
        if frame.ndim == 2:
            frame = frame.mean(axis=1)
        result = []
        # Split even unusually large input blocks at exact sample boundaries.
        while len(frame):
            count = min(len(frame), self.next - self.end)
            self.audio = np.concatenate((self.audio, frame[:count]))
            frame = frame[count:]
            self.end += count
            if self.end == self.next:
                final = self.end == (self.epoch + 1) * self.core
                result.append(self.snapshot(final))
                if final:
                    self.epoch += 1
                    self.audio = self.audio[-self.overlap:].copy() if self.overlap else np.empty(0, np.float32)
                    self.origin = self.end - len(self.audio)
                self.next += self.hop
        return result

    def snapshot(self, final):
        return Window(self.epoch, self.origin, self.end, final, self.audio.copy())

    def tail(self):
        if self.end > self.epoch * self.core:
            return self.snapshot(True)
        return None


class PendingWindows:
    """Replace obsolete partials, retain boundaries; fail explicitly on overload."""
    def __init__(self):
        self.items = deque()
        self.condition = threading.Condition()
        self.closed = False

    def put(self, window):
        with self.condition:
            if self.closed:
                raise RuntimeError("ASR queue is closed")
            self.items = deque(w for w in self.items if w.epoch != window.epoch or w.final)
            if len(self.items) >= 3:
                raise RuntimeError("ASR 推理持续落后于采集，请使用 Kotoba、快捷模式或检查 GPU 加速")
            self.items.append(window)
            self.condition.notify()

    def get(self):
        with self.condition:
            self.condition.wait_for(lambda: self.items or self.closed)
            return self.items.popleft() if self.items else None

    def close(self, discard=False):
        with self.condition:
            self.closed = True
            if discard:
                self.items.clear()
            self.condition.notify_all()


def normalized(text):
    chars, offsets = [], []
    for i, char in enumerate(text):
        for expanded in unicodedata.normalize("NFKC", char):
            if unicodedata.category(expanded)[0] in "LN":
                chars.append(expanded)
                offsets.append(i)
    return "".join(chars), offsets


def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        next_row = [i]
        for j, y in enumerate(b, 1):
            next_row.append(min(next_row[-1] + 1, row[j] + 1, row[j - 1] + (x != y)))
        row = next_row
    return row[-1]


def anchor(old, new):
    if not old:
        return 0
    if new.startswith(old):
        return len(old)
    return min(range(max(0, len(old) - 8), min(len(new), len(old) + 8) + 1),
               key=lambda i: (distance(old, new[:i]), abs(i - len(old))), default=len(new))


def overlap(past, current):
    limit = min(len(past), len(current), 80)
    for k in range(limit, 2, -1):
        if past[-k:] == current[:k]:
            return k
    choices = []
    for k in range(6, min(limit, 60) + 1):
        edits = distance(past[-k:], current[:k])
        if edits / k <= 0.30:
            choices.append((k - edits, k))
    return max(choices, default=(0, 0))[1]


class Agreement:
    def __init__(self):
        self.epoch = -1
        self.previous = self.local = self.total = self.past = ""

    def push(self, epoch, text, final):
        if epoch != self.epoch:
            self.epoch = epoch
            self.previous = self.local = ""
            self.past = self.total[-100:]
        current, offsets = normalized(text)
        cut = overlap(self.past, current) if epoch else 0
        current = current[cut:]
        start = anchor(self.local, current)
        old_start = anchor(self.local, self.previous)
        stable = start
        for a, b in zip(self.previous[old_start:], current[start:]):
            if a != b:
                break
            stable += 1
        if final:
            stable = len(current)
        self.previous = current
        candidate = current[:stable]
        begin = anchor(self.local, candidate)
        if stable <= begin:
            return ""
        self.local = candidate
        self.total = (self.total + candidate[begin:])[-200:]
        # Keep original kanji, kana and punctuation; normalization is alignment only.
        raw_start = offsets[cut + begin] if cut + begin else 0
        raw_end = offsets[cut + stable] if cut + stable < len(offsets) else len(text)
        return text[raw_start:raw_end].strip()


class Recognizer:
    def __init__(self, model, model_id, mode, emit):
        self.model, self.model_id, self.mode, self.emit = model, model_id, mode, emit
        self.agreement = Agreement()
        self.pending = ""
        self.pending_at = None
        self.last_commit = self.end_ms = self.latency = 0

    def process(self, window):
        self.end_ms = window.end * 1000 // RATE
        started = time.perf_counter()
        text = ""
        # Light energy floor, deliberately no Silero gate or decoder VAD pass.
        voiced = len(window.audio) >= RATE // 5 and float(np.sqrt(np.mean(window.audio ** 2))) >= 0.001
        if voiced:
            segments, _ = self.model.transcribe(
                window.audio, language="ja", task="transcribe", beam_size=3,
                best_of=1, condition_on_previous_text=False, vad_filter=False,
            )
            text = "".join(segment.text for segment in segments).strip()
        self.latency = int((time.perf_counter() - started) * 1000)
        if text:
            self.emit("partial", text=text, start_ms=window.start * 1000 // RATE,
                      end_ms=self.end_ms, latency_ms=self.latency, model_id=self.model_id)
        delta = text if self.mode == "quick" else self.agreement.push(window.epoch, text, window.final)
        if delta:
            if not self.pending:
                self.pending_at = self.end_ms
            self.pending += delta
        age = self.end_ms - (self.pending_at if self.pending_at is not None else self.end_ms)
        size = len(normalized(self.pending)[0])
        if window.final or size >= 8 or (age >= 2000 and size >= 3) or age >= 4000:
            self.flush()
        if not voiced and window.final:
            # No textual history across an entire silent window.
            self.agreement = Agreement()
        if not self.pending and not text:
            self.last_commit = self.end_ms

    def flush(self):
        if self.pending:
            self.emit("final", text=self.pending, start_ms=self.last_commit,
                      end_ms=self.end_ms, latency_ms=self.latency, model_id=self.model_id)
            self.last_commit = self.end_ms
            self.pending = ""
            self.pending_at = None
