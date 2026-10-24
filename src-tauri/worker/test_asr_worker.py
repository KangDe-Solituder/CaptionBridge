import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np

from asr_worker import (
    AcousticEvidenceTracker,
    CUDA_RUNTIME_FILES,
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
    def test_cross_talk_does_not_block_a_sustained_better_ear(self) -> None:
        for first, second in ((0, 1), (1, 0)):
            router = StereoRouter("auto", 0.3)
            initial = np.array([0.08, 0.08, 0.48])
            initial[first] = 0.95
            for _ in range(20):
                router.process(initial)
            changed = np.array([0.4, 0.4, 0.65])
            changed[second] = 0.95
            handoffs = []
            for _ in range(100):
                _, _, handoff = router.process(changed)
                if handoff:
                    handoffs.append(handoff)
            self.assertEqual(router.selected, ("left", "right")[second])
            self.assertEqual(len(handoffs), 1)

    def test_other_speaker_is_not_learned_as_a_permanent_noise_floor(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)
        for _ in range(100):
            router.process(np.array([0.95, 0.72, 0.60]))
        for _ in range(40):
            router.process(np.array([0.01, 0.72, 0.50]))
        self.assertEqual(router.selected, "right")

    def test_strong_speech_can_override_a_stale_high_baseline(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)
        router.noise_baseline[1] = 0.8
        router.noise_baseline_ready[1] = True
        for _ in range(40):
            router.process(np.array([0.01, 0.72, 0.50]))
        self.assertEqual(router.selected, "right")

    def test_mix_can_release_even_while_it_still_contains_speech(self) -> None:
        router = StereoRouter("auto", 0.3)
        for _ in range(20):
            router.process(np.array([0.9, 0.9, 0.95]))
        self.assertEqual(router.selected, "mix")
        for _ in range(30):
            router.process(np.array([0.1, 0.95, 0.94]))
        self.assertEqual(router.selected, "right")

    def test_mix_can_release_when_equal_ears_cancel_in_the_downmix(self) -> None:
        router = StereoRouter("auto", 0.3)
        for _ in range(20):
            router.process(np.array([0.9, 0.9, 0.95]))
        for _ in range(30):
            router.process(np.array([0.9, 0.9, 0.02]))
        self.assertIn(router.selected, {"left", "right"})

    def test_alternating_one_frame_spikes_do_not_steal_lock(self) -> None:
        router = StereoRouter("auto", 0.3)
        lock_left(router)
        for index in range(100):
            router.process(np.array([0.4, 0.98 if index % 2 else 0.02, 0.3]))
        self.assertEqual(router.selected, "left")

    def test_nan_frame_does_not_poison_future_routing(self) -> None:
        router = StereoRouter("auto", 0.3)
        router.process(np.array([np.nan, np.nan, np.nan]))
        lock_left(router)

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
    def test_steady_tonal_evidence_is_not_rejected_only_for_flat_volume(self) -> None:
        tracker = AcousticEvidenceTracker()
        t = np.arange(1600, dtype=np.float32) / 16000.0
        signal = np.sin(2 * np.pi * 180 * t).astype(np.float32) * 0.08
        for _ in range(40):
            evidence = tracker.process(np.stack((signal, signal, signal)), np.full(3, 0.95))
        # This tests the acoustic weighting with a controlled VAD input, not
        # whether a real VAD would classify the synthetic tone as speech.
        self.assertGreater(evidence[0], StereoRouter("auto", 0.3).threshold)

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

    @patch("asr_worker.ctypes.WinDLL", side_effect=lambda path: path)
    @patch("asr_worker.os.add_dll_directory", side_effect=lambda path: path)
    def test_gpu_runtime_is_preloaded_before_ctranslate2_import(
        self,
        add_dll_directory,
        win_dll,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            for name in CUDA_RUNTIME_FILES + CUDNN_RUNTIME_FILES:
                runtime.joinpath(name).touch()
            with patch.dict(
                os.environ,
                {"LIVECAPTION_CUDA_RUNTIME_DIR": str(runtime), "PATH": ""},
            ):
                worker = Worker()
                worker.preload_gpu_runtime()

        self.assertTrue(worker.gpu_runtime_preloaded)
        self.assertEqual(len(worker.runtime_dll_handles), 10)
        add_dll_directory.assert_called()
        self.assertEqual(win_dll.call_count, 10)


class WorkerRoutingTests(unittest.TestCase):
    def test_handoff_includes_smoothing_delay_but_preserves_previous_word(self) -> None:
        router = StereoRouter("auto", 0.3)
        scores = [np.array([.9, .1, .4])] * 4 + [np.array([.4, .95, .65])] * 12
        span = router.handoff_span(scores, "left", "right")
        self.assertEqual(span, 12)
        worker = Worker()
        old = np.column_stack((np.ones(1600, dtype=np.float32), np.zeros(1600, dtype=np.float32)))
        new = np.column_stack((np.zeros(1600, dtype=np.float32), np.full(1600, .8, dtype=np.float32)))
        worker.utterance = [StereoChunk(old.copy(), (1., 0.)) for _ in range(4)]
        worker.utterance += [StereoChunk(new.copy(), (1., 0.)) for _ in range(11)]
        worker.apply_channel_handoff("left", "right", span - 1)
        audio, _, _ = worker.snapshot()
        np.testing.assert_array_equal(audio[:6400], np.ones(6400, dtype=np.float32))
        np.testing.assert_allclose(audio[6880:], .8)

    def test_zero_history_handoff_does_not_rewrite_the_entire_utterance(self) -> None:
        worker = Worker()
        worker.utterance = [StereoChunk(np.ones((1600, 2), dtype=np.float32), (1., 0.))]
        worker.apply_channel_handoff("left", "right", 0)
        self.assertEqual(worker.utterance[0].weights, (1., 0.))

    def test_handoff_restores_new_ear_onset_with_only_a_30ms_crossfade(self) -> None:
        worker = Worker()
        stereo = np.column_stack((np.zeros(1600, dtype=np.float32), np.ones(1600, dtype=np.float32)))
        worker.utterance = [
            StereoChunk(stereo.copy(), StereoRouter.weights("left"))
            for _ in range(3)
        ]

        worker.apply_channel_handoff("left", "right", 3)
        audio, _, _ = worker.snapshot()
        self.assertEqual(audio[0], 0.0)
        self.assertGreater(audio[240], 0.49)
        np.testing.assert_array_equal(audio[480:], np.ones(4320, dtype=np.float32))
        np.testing.assert_array_equal(worker.utterance[0].frame, stereo)

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
