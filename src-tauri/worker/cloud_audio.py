"""Bounded causal upload windows. No model, credentials or networking in the worker."""
import base64
import io
import wave
import numpy as np
from streaming_asr import RATE, Window

class CloudWindows:
    def __init__(self):
        self.end = self.start = self.origin = self.epoch = self.quiet = 0
        self.audio = np.empty(0, np.float32)
        self.remainder = np.empty(0, np.float32)

    def feed(self, frame):
        frame = np.asarray(frame, dtype=np.float32)
        if frame.ndim == 2: frame = frame.mean(axis=1)
        self.remainder = np.concatenate((self.remainder, frame))
        result = []
        while len(self.remainder) >= 320:
            part, self.remainder = self.remainder[:320], self.remainder[320:]
            self.audio = np.concatenate((self.audio, part)); self.end += len(part)
            self.quiet = self.quiet + len(part) if float(np.sqrt(np.mean(part ** 2))) < .0005 else 0
            pause = self.end - self.start >= 2.5 * RATE and self.quiet >= .36 * RATE
            hard = self.end - self.start >= 6 * RATE
            if pause or hard:
                result.append(Window(self.epoch, self.origin, self.end, True, self.audio.copy()))
                self.epoch += 1; self.start = self.end; self.quiet = 0
                overlap = int(.3 * RATE) if hard and not pause else 0
                self.audio = self.audio[-overlap:].copy() if overlap else np.empty(0, np.float32)
                self.origin = self.end - len(self.audio)
        return result

    def tail(self):
        if len(self.remainder):
            self.audio = np.concatenate((self.audio, self.remainder));self.end += len(self.remainder)
            self.remainder = np.empty(0, np.float32)
        if self.end <= self.start: return None
        result = Window(self.epoch, self.origin, self.end, True, self.audio.copy())
        self.start = self.end
        return result

class AudioEmitter:
    def __init__(self, emit): self.emit = emit
    def process(self, window):
        output = io.BytesIO()
        with wave.open(output, 'wb') as f:
            f.setnchannels(1); f.setsampwidth(2); f.setframerate(RATE)
            f.writeframes(np.clip(np.rint(window.audio * 32768), -32768, 32767).astype('<i2').tobytes())
        self.emit('audio', wav_base64=base64.b64encode(output.getvalue()).decode('ascii'),
                  start_ms=window.start * 1000 // RATE, end_ms=window.end * 1000 // RATE)
    def flush(self): pass
