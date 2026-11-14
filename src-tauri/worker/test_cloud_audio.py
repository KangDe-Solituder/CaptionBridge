import base64
import io
import wave
import unittest
from unittest.mock import patch
import numpy as np
from cloud_audio import CloudWindows, AudioEmitter, RATE
from asr_worker import Worker


class CloudAudioTests(unittest.TestCase):
    def test_ten_minutes_upload_is_bounded_and_covers_every_sample(self):
        scheduler = CloudWindows()
        windows = []
        for _ in range(6000):
            windows.extend(scheduler.feed(np.full(1600, .01, np.float32)))
        self.assertIsNone(scheduler.tail())
        self.assertEqual(len(windows), 100)
        self.assertEqual(windows[0].start, 0)
        self.assertEqual(windows[-1].end, 600 * RATE)
        self.assertLess(sum(len(w.audio) for w in windows), 630 * RATE)
        for previous, current in zip(windows, windows[1:]):
            self.assertEqual(previous.end - current.start, int(.3 * RATE))
            self.assertLessEqual(len(current.audio), int(6.3 * RATE))

    def test_pause_cut_and_tail_are_lossless_and_idempotent(self):
        scheduler = CloudWindows()
        audio = np.concatenate((np.full(3 * RATE, .01), np.zeros(RATE), np.full(97, .01)))
        windows = scheduler.feed(audio)
        windows.append(scheduler.tail())
        self.assertEqual(windows[0].end, int(3.36 * RATE))
        self.assertIsNone(scheduler.tail())
        np.testing.assert_allclose(np.concatenate([w.audio for w in windows]), audio)

    def test_stereo_mix_and_pcm_protocol(self):
        scheduler = CloudWindows()
        stereo = np.column_stack((np.full(RATE, .2), np.zeros(RATE)))
        scheduler.feed(stereo)
        events = []
        AudioEmitter(lambda kind, **data: events.append((kind, data))).process(scheduler.tail())
        kind, data = events[0]
        self.assertEqual(kind, 'audio')
        self.assertEqual(data['end_ms'], 1000)
        with wave.open(io.BytesIO(base64.b64decode(data['wav_base64']))) as f:
            self.assertEqual((f.getframerate(), f.getnchannels(), f.getsampwidth()), (16000, 1, 2))
            self.assertAlmostEqual(float(np.frombuffer(f.readframes(RATE), '<i2').mean()) / 32768, .1, places=4)

    def test_cloud_starts_without_model_and_stop_drains_tail(self):
        worker = Worker()
        events = []
        def fake_capture():
            for w in worker.windows.feed(np.full(RATE, .01, np.float32)):
                worker.pending_windows.put(w)
        with patch('asr_worker.emit', lambda kind, **data: events.append((kind, data))), patch.object(worker, 'capture_loop_safe', fake_capture):
            worker.start(cloud=True)
            worker.stop()
        self.assertIsNone(worker.model)
        self.assertFalse(worker.capture_thread.is_alive())
        self.assertFalse(worker.inference_thread.is_alive())
        self.assertEqual([kind for kind, _ in events], ['audio', 'health'])

if __name__ == '__main__':
    unittest.main()
