import unittest
import numpy as np
from consensus_asr import Consensus, ConsensusWindows, Unit, chars, norm
from streaming_asr import RATE


class ConsensusTests(unittest.TestCase):
    def test_consistency_rule(self):
        for inputs, expected in [(['a','a','b'],'a'), (['a','b','b'],'b'),
                                 (['a','b','a'],'a'), (['a','b','c'],'a'),
                                 (['a','',''],'a'), (['','b','b'],'b')]:
            with self.subTest(inputs=inputs):
                unit = Unit()
                for text in inputs:
                    unit.observe(text)
                self.assertEqual(unit.selected, expected)
                self.assertTrue(unit.frozen)

    def test_eof_retains_first_and_surface_form(self):
        unit = Unit()
        unit.observe('Ａ、猫。')
        unit.observe('B猫')
        self.assertEqual(unit.selected, 'Ａ、猫。')
        word = dict(a=0,b=1,t=' Ａ、猫。')
        self.assertEqual(''.join(c['raw'] for c in chars([word])), word['t'])
        unit.observe('B猫')
        self.assertEqual(unit.selected, 'B猫')

    def test_audio_windows_and_tail(self):
        windows = ConsensusWindows()
        result = windows.feed(np.zeros((21*RATE,2), np.float32))
        self.assertEqual([(w.start/RATE,w.end/RATE) for w in result], [(0,8),(0,12),(4,16),(8,20)])
        tail = windows.tail()
        self.assertEqual((tail.start/RATE,tail.end/RATE), (9,21))
        self.assertLessEqual(len(windows.audio),12*RATE)

    def test_no_duplicate_tail_at_exact_boundary(self):
        windows = ConsensusWindows()
        windows.feed(np.zeros(12*RATE,np.float32))
        self.assertIsNone(windows.tail())

    def test_group_replaces_and_unmodified_text_does_not_emit(self):
        state = Consensus()
        words = [dict(a=1,b=2,t='今日は。'),dict(a=5,b=6,t='晴れです。')]
        first = state.push(words,0,8)
        self.assertEqual(len(first),1)
        self.assertEqual(first[0]['text'],'今日は。晴れです。')
        self.assertEqual(state.push(words,0,12),[])
        self.assertEqual(state.push(words[1:],4,16),[])

    def test_state_stays_bounded_and_output_ordered(self):
        state = Consensus()
        rows = {}
        for end in range(8,1201,4):
            words = [dict(a=i*4+.2,b=i*4+1,t=f'単語{i}。')
                     for i in range(max(0,end//4-3),end//4)]
            for event in state.push(words,max(0,end-12),end):
                rows[event['group_id']] = event['text']
            self.assertLessEqual(len(state.units),9)
            self.assertLessEqual(len(state.previous),80)
            self.assertLessEqual(len(state.backbone),100)
        joined=norm(''.join(rows[i] for i in sorted(rows)))
        expected=norm(''.join(f'単語{i}。' for i in range(300)))
        self.assertEqual(joined,expected)


if __name__ == '__main__':
    unittest.main()
