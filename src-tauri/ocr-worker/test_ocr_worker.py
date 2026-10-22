import unittest
from ocr_worker import group_lines, validate_crop


def line(text, x, y, w=100, h=20):
    return {'text': text, 'bounds': [x, y, w, h], 'confidence': 0.9}


class LayoutTests(unittest.TestCase):
    def test_paragraphs_do_not_merge_disconnected_columns(self):
        blocks = group_lines([line('左一', 0, 0), line('右一', 250, 0),
                              line('左二', 0, 27), line('右二', 250, 27),
                              line('提示', 0, 150)])
        self.assertEqual([b['text'] for b in blocks], ['左一\n左二', '右一\n右二', '提示'])
        self.assertEqual(blocks[0]['bounds'], [0, 0, 100, 47])

    def test_heading_kept_separate_and_long_text_is_bounded(self):
        blocks = group_lines([line('标题', 0, 0, 100, 40), line('正文', 0, 45)])
        self.assertEqual(len(blocks), 2)
        blocks = group_lines([line('🙂' * 1300, 0, 0)])
        self.assertEqual([len(b['text']) for b in blocks], [600, 600, 100])
        self.assertEqual(''.join(b['text'] for b in blocks), '🙂' * 1300)

    def test_crop_never_reads_outside_image(self):
        self.assertEqual(validate_crop([20, 30, 80, 70], 100, 100), (20, 30, 100, 100))
        for rect in ([-1, 0, 20, 20], [0, 0, 101, 20], [0, 0, 2, 2], [0, 0, 20.5, 20]):
            with self.assertRaises(ValueError):
                validate_crop(rect, 100, 100)


if __name__ == '__main__':
    unittest.main()
