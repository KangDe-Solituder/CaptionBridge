import threading
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np
from streaming_asr import Agreement, PendingWindows, RATE, Recognizer, Window, Windows, overlap
from asr_worker import Worker


class StreamingTests(unittest.TestCase):
    def test_windows_keep_long_context_and_sample_clock(self):
        scheduler = Windows()
        windows = scheduler.feed(np.ones(26 * RATE, np.float32))
        self.assertEqual([w.end // RATE for w in windows], list(range(2, 27, 2)))
        self.assertEqual([(w.start // RATE, w.end // RATE) for w in windows if w.final], [(0, 12), (8, 24)])
        self.assertLessEqual(max(len(w.audio) for w in windows), 16 * RATE)
        self.assertEqual(scheduler.tail().end, 26 * RATE)

    def test_quick_exact_boundaries_and_short_tail(self):
        scheduler = Windows("quick")
        windows = scheduler.feed(np.ones(9 * RATE, np.float32))
        self.assertEqual([(w.start, w.end, w.final) for w in windows], [(0, 4 * RATE, True), (4 * RATE, 8 * RATE, True)])
        self.assertEqual(len(scheduler.tail().audio), RATE)
        self.assertIsNone(Windows().tail())
        exact = Windows()
        exact.feed(np.ones(12 * RATE, np.float32))
        self.assertIsNone(exact.tail())

    def test_mix_immediately_follows_either_ear(self):
        scheduler = Windows("quick")
        stereo = np.zeros((8 * RATE, 2), np.float32)
        stereo[:4 * RATE, 0] = .1
        stereo[4 * RATE:, 1] = .2
        a, b = scheduler.feed(stereo)
        np.testing.assert_allclose(a.audio, .05)
        np.testing.assert_allclose(b.audio, .1)

    def test_agreement_preserves_punctuation_and_only_commits_new_suffix(self):
        policy = Agreement()
        self.assertEqual(policy.push(0, "今日は、いい天気です。", False), "")
        self.assertEqual(policy.push(0, "今日は、いい天気です。散歩", False), "今日は、いい天気です。")
        self.assertEqual(policy.push(0, "今日は、いい天気です。散歩しましょう。", False), "散歩")
        self.assertEqual(policy.push(0, "今日は、いい天気です。散歩しましょう。", True), "しましょう。")
        # Repeated audio is context, not another final subtitle.
        self.assertEqual(policy.push(1, "散歩しましょう。次の話です。", False), "")
        self.assertEqual(policy.push(1, "散歩しましょう。次の話です。", True), "次の話です。")

    def test_old_kanji_revision_does_not_block_new_words(self):
        policy = Agreement()
        policy.push(0, "水瀬さんこんにちは", False)
        self.assertEqual(policy.push(0, "水瀬さんこんにちは", False), "水瀬さんこんにちは")
        self.assertEqual(policy.push(0, "みなせさんこんにちは今日は", False), "")
        self.assertEqual(policy.push(0, "みなせさんこんにちは今日は楽しい", False), "今日は")

    def test_overlap_exact_priority_and_no_single_character_dedup(self):
        self.assertEqual(overlap("前の話こんにちは", "こんにちは次の話"), 5)
        self.assertEqual(overlap("はい", "いろいろ"), 0)

    def test_queue_coalesces_partials_but_retains_boundaries(self):
        queue = PendingWindows()
        empty = np.empty(0, np.float32)
        for end in (2, 4, 6):
            queue.put(Window(0, 0, end, False, empty))
        queue.put(Window(0, 0, 12, True, empty))
        queue.put(Window(1, 8, 14, False, empty))
        queue.close()
        self.assertTrue(queue.get().final)
        self.assertEqual(queue.get().epoch, 1)
        self.assertIsNone(queue.get())

    def test_overload_is_explicit_not_unbounded_memory_or_silent_audio_loss(self):
        queue = PendingWindows()
        for epoch in range(3):
            queue.put(Window(epoch, 0, 0, True, np.empty(0)))
        with self.assertRaisesRegex(RuntimeError, "ASR"):
            queue.put(Window(3, 0, 0, True, np.empty(0)))

    def test_soft_whisper_reaches_decoder_without_vad_and_silence_does_not(self):
        calls, events = [], []
        def transcribe(audio, **kwargs):
            calls.append(kwargs)
            return [SimpleNamespace(text="ごめん。")], None
        recognizer = Recognizer(SimpleNamespace(transcribe=transcribe), "test", "quick", lambda kind, **data: events.append((kind, data)))
        recognizer.process(Window(0, 0, 4 * RATE, True, np.full(4 * RATE, .002, np.float32)))
        recognizer.process(Window(1, 4 * RATE, 8 * RATE, True, np.zeros(4 * RATE, np.float32)))
        self.assertEqual(len(calls), 1)
        self.assertFalse(calls[0]["vad_filter"])
        self.assertFalse(calls[0]["condition_on_previous_text"])
        self.assertEqual(calls[0]["beam_size"], 3)
        self.assertEqual([data["text"] for kind, data in events if kind == "final"], ["ごめん。"])

    def test_short_stable_text_waits_but_does_not_wait_for_full_window(self):
        events = []
        model = SimpleNamespace(transcribe=lambda *args, **kw: ([SimpleNamespace(text="はい。")], None))
        recognizer = Recognizer(model, "test", "default", lambda kind, **data: events.append((kind, data)))
        for window in Windows().feed(np.full(8 * RATE, .01, np.float32)):
            recognizer.process(window)
        finals = [data for kind, data in events if kind == "final"]
        self.assertEqual(len(finals), 1)
        self.assertEqual(finals[0]["text"], "はい。")
        self.assertEqual(finals[0]["end_ms"], 8000)

    def test_stop_drains_tail_before_ack_and_restart_has_no_old_text(self):
        worker = Worker()
        worker.model_id = "test"
        worker.model = SimpleNamespace(transcribe=lambda *args, **kw: ([SimpleNamespace(text="最後の言葉です。")], None))
        events = []
        def capture():
            worker.windows.feed(np.full(RATE, .01, np.float32))
        with patch.object(worker, "capture_loop_safe", capture), patch("asr_worker.emit", side_effect=lambda kind, **data: events.append((kind, data))):
            for _ in range(2):
                worker.start()
                worker.capture_thread.join()
                worker.stop()
                self.assertFalse(worker.inference_thread.is_alive())
        self.assertEqual([kind for kind, _ in events], ["partial", "final", "health"] * 2)
        self.assertEqual(events[2][1]["detail"], "capture_stopped")
        self.assertEqual(events[1][1]["start_ms"], events[4][1]["start_ms"])

    def test_capture_continues_while_inference_is_blocked(self):
        entered, release, captured = threading.Event(), threading.Event(), threading.Event()
        worker = Worker()
        worker.model_id = "test"
        def transcribe(*args, **kwargs):
            entered.set()
            release.wait(3)
            return [SimpleNamespace(text="テストです。")], None
        worker.model = SimpleNamespace(transcribe=transcribe)
        def capture():
            for window in worker.windows.feed(np.full(2 * RATE, .01, np.float32)):
                worker.pending_windows.put(window)
            if entered.wait(2):
                for window in worker.windows.feed(np.full(4 * RATE, .01, np.float32)):
                    worker.pending_windows.put(window)
                captured.set()
        with patch.object(worker, "capture_loop_safe", capture), patch("asr_worker.emit"):
            worker.start()
            try:
                self.assertTrue(captured.wait(2), "Capture waited for the model")
                self.assertFalse(release.is_set())
                self.assertEqual(worker.windows.end, 6 * RATE)
            finally:
                release.set()
                worker.stop()


if __name__ == "__main__":
    unittest.main()
