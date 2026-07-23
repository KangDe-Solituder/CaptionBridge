import unittest

import numpy as np

from asr_worker import (
    AcousticEvidenceTracker,
    CUDNN_RUNTIME_FILES,
    StereoChunk,
    StereoRouter,
    Worker,
)


def lock_left(router: StereoRouter) -> None:
    for _ in range(router.initial_confirm_frames + 8):
        router.process(np.array([0.92, 0.08, 0.48]))
    assert router.selected == "left"


class StereoRouterTests(unittest.TestCase):
    def test_silence_never_releases_the_locked_channel(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)

        for _ in range(80):
            route, probability, handoff = router.process(np.zeros(3))

        self.assertEqual(route, "left")
        self.assertEqual(router.selected, "left")
        self.assertEqual(probability, 0.0)
        self.assertIsNone(handoff)

    def test_stationary_noise_on_other_side_does_not_take_lock_during_pause(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)

        # Learn that the right-side activity is the standing noise floor while
        # the selected left-side speaker is active.
        for _ in range(20):
            router.process(np.array([0.9, 0.52, 0.48]))
        for _ in range(30):
            route, _, handoff = router.process(np.array([0.02, 0.52, 0.28]))

        self.assertEqual(route, "left")
        self.assertEqual(router.selected, "left")
        self.assertIsNone(handoff)

    def test_new_sustained_speech_evidence_switches_sides(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)
        for _ in range(20):
            router.process(np.array([0.9, 0.30, 0.45]))

        handoff = None
        for _ in range(router.switch_confirm_frames + 12):
            route, _, handoff = router.process(np.array([0.02, 0.95, 0.48]))
            if handoff:
                break

        self.assertEqual(route, "right")
        self.assertEqual(router.selected, "right")
        self.assertEqual(handoff, ("left", "right", router.switch_confirm_frames))

    def test_one_frame_opposite_side_noise_does_not_switch(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)

        route, _, handoff = router.process(np.array([0.02, 0.98, 0.50]))

        self.assertEqual(route, "left")
        self.assertIsNone(handoff)

    def test_initial_search_gates_audio_until_lock_is_confirmed(self) -> None:
        router = StereoRouter("auto", 0.3)
        for _ in range(router.initial_confirm_frames - 1):
            route, probability, handoff = router.process(np.array([0.9, 0.05, 0.46]))
            self.assertEqual(route, "left")
            self.assertEqual(probability, 0.0)
            self.assertIsNone(handoff)

        handoff = None
        for _ in range(8):
            _, probability, handoff = router.process(np.array([0.9, 0.05, 0.46]))
            if handoff:
                break
        self.assertEqual(router.selected, "left")
        self.assertGreater(probability, 0.0)
        self.assertEqual(handoff[0], "searching")

    def test_reset_is_the_only_way_to_return_to_searching(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)
        router.reset()

        self.assertIsNone(router.selected)
        self.assertEqual(router.status, "asmr_searching")

    def test_manual_modes_do_not_auto_switch(self) -> None:
        for mode in ("left", "right", "mix"):
            router = StereoRouter(mode, 0.3)
            route, _, handoff = router.process(np.array([0.05, 0.95, 0.5]))
            self.assertEqual(route, mode)
            self.assertIsNone(handoff)

    def test_sensitivity_presets_have_ordered_confirmation_times(self) -> None:
        stable = StereoRouter("auto", 0.3, "stable")
        standard = StereoRouter("auto", 0.3, "standard")
        responsive = StereoRouter("auto", 0.3, "responsive")

        self.assertGreater(stable.switch_confirm_frames, standard.switch_confirm_frames)
        self.assertGreater(standard.switch_confirm_frames, responsive.switch_confirm_frames)


class AcousticEvidenceTests(unittest.TestCase):
    def test_stationary_white_noise_scores_below_modulated_voice_like_audio(self) -> None:
        tracker = AcousticEvidenceTracker()
        rng = np.random.default_rng(7)
        t = np.arange(1600, dtype=np.float32) / 16000.0
        evidence = np.zeros(3)
        for index in range(14):
            voice = np.sin(2 * np.pi * 180 * t).astype(np.float32)
            voice *= 0.03 + 0.17 * (index % 5) / 4
            noise = rng.normal(0, 0.08, 1600).astype(np.float32)
            center = (voice + noise) * 0.5
            evidence = tracker.process(
                np.stack((voice, noise, center)),
                np.array([0.9, 0.9, 0.9]),
            )

        self.assertGreater(evidence[0], evidence[1] + 0.12)


class DependencyProbeTests(unittest.TestCase):
    def test_cudnn_probe_requires_the_full_inference_runtime(self) -> None:
        self.assertEqual(len(CUDNN_RUNTIME_FILES), 8)
        self.assertIn("cudnn_ops64_9.dll", CUDNN_RUNTIME_FILES)
        self.assertIn("cudnn_graph64_9.dll", CUDNN_RUNTIME_FILES)


class WorkerRoutingTests(unittest.TestCase):
    def test_handoff_crossfades_recent_stereo_chunks(self) -> None:
        worker = Worker()
        stereo = np.column_stack((np.ones(4, dtype=np.float32), np.zeros(4, dtype=np.float32)))
        worker.utterance = [
            StereoChunk(stereo.copy(), StereoRouter.weights("left"))
            for _ in range(3)
        ]

        worker.apply_channel_handoff("left", "right", 3)
        left_weights = [chunk.weights[0] for chunk in worker.utterance]
        self.assertGreater(left_weights[0], left_weights[1])
        self.assertGreater(left_weights[1], left_weights[2])

    def test_snapshot_renders_only_the_selected_mono_route(self) -> None:
        worker = Worker()
        worker.stream_started = 1.0
        worker.utterance_started = 1.0
        stereo = np.column_stack(
            (np.ones(4, dtype=np.float32), np.zeros(4, dtype=np.float32))
        )
        worker.utterance = [
            StereoChunk(stereo.copy(), StereoRouter.weights("left")),
            StereoChunk(stereo.copy(), StereoRouter.weights("right")),
        ]

        audio, _, _ = worker.snapshot()
        np.testing.assert_array_equal(audio[:4], np.ones(4, dtype=np.float32))
        np.testing.assert_array_equal(audio[4:], np.zeros(4, dtype=np.float32))

    def test_non_speech_gate_requires_sustained_voice_evidence_in_auto_mode(self) -> None:
        worker = Worker()
        worker.profile.update({
            "channel_mode": "auto",
            "suppress_non_speech_segments": True,
            "threshold": 0.3,
        })
        worker.utterance_voice_frames = 3
        worker.utterance_probability_sum = 2.4
        self.assertFalse(worker.segment_has_speech())

        worker.utterance_voice_frames = 5
        worker.utterance_probability_sum = 3.5
        self.assertTrue(worker.segment_has_speech())


if __name__ == "__main__":
    unittest.main()
