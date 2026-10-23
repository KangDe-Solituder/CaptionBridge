import io
import json
import unittest
from ocr_worker import MAX_BLOCK_CHARS, group_lines, serve, validate_crop


def line(text, x, y, w=100, h=20):
    return {'text': text, 'bounds': [x, y, w, h], 'confidence': 0.9}


class LayoutTests(unittest.TestCase):
    def test_paragraphs_do_not_merge_disconnected_columns(self):
        blocks = group_lines([line('左一', 0, 0), line('右一', 250, 0),
                              line('左二', 0, 27), line('右二', 250, 27),
                              line('提示', 0, 150)])
        # Read the entire left column before the right column.
        self.assertEqual([b['text'] for b in blocks], ['左一\n左二', '提示', '右一\n右二'])
        self.assertEqual(blocks[0]['bounds'], [0, 0, 100, 47])

    def test_full_width_heading_and_footer_surround_columns(self):
        blocks = group_lines([line('跨栏标题', 0, 0, 500, 32),
                              line('左栏第一段', 0, 65, 200), line('右栏第一段', 300, 64, 200),
                              line('左栏第一段续行', 0, 95, 200), line('右栏第一段续行', 300, 94, 200),
                              line('左栏第二段', 0, 170, 200), line('右栏第二段', 300, 169, 200),
                              line('跨栏页脚内容', 0, 240, 500)])
        self.assertEqual([b['text'] for b in blocks], ['跨栏标题', '左栏第一段\n左栏第一段续行', '左栏第二段',
                                                     '右栏第一段\n右栏第一段续行', '右栏第二段', '跨栏页脚内容'])

    def test_heading_kept_separate_and_long_text_is_bounded(self):
        blocks = group_lines([line('标题', 0, 0, 100, 40), line('正文', 0, 45)])
        self.assertEqual(len(blocks), 2)
        text = '🙂' * (MAX_BLOCK_CHARS * 2 + 100)
        blocks = group_lines([line(text, 0, 0)])
        self.assertEqual([len(b['text']) for b in blocks], [MAX_BLOCK_CHARS, MAX_BLOCK_CHARS, 100])
        self.assertEqual(''.join(b['text'] for b in blocks), text)

    def test_indented_paragraph_with_wide_line_spacing(self):
        rows = [line('首行缩进', 40, 0, 360), line('自然换行', 0, 44, 400),
                line('同一段的末行。', 0, 88, 240), line('下一段首行', 40, 132, 360),
                line('下一段的末行。', 0, 176, 200)]
        blocks = group_lines(rows)
        self.assertEqual([b['text'] for b in blocks], ['首行缩进\n自然换行\n同一段的末行。', '下一段首行\n下一段的末行。'])
        self.assertEqual(blocks[0]['bounds'], [0, 0, 400, 108])

    def test_blank_line_and_spacing_increase_break_paragraph(self):
        blocks = group_lines([line('one', 0, 0, 300), line('two', 0, 28, 300),
                              line('new paragraph', 0, 76, 300)])
        self.assertEqual(len(blocks), 2)

    def test_short_last_line_separates_next_paragraph_without_indent(self):
        blocks = group_lines([line('first full line', 0, 0, 400), line('last sentence.', 0, 30, 200),
                              line('Next paragraph starts here', 0, 60, 400)])
        self.assertEqual(len(blocks), 2)

    def test_list_items_stay_separate_but_wrapped_continuation_joins(self):
        blocks = group_lines([line('1. First item wraps', 0, 0, 350), line('onto a second line', 30, 32, 300),
                              line('2. Second item', 0, 64, 350), line('its continuation', 30, 96, 300)])
        self.assertEqual([b['text'] for b in blocks], ['1. First item wraps\nonto a second line', '2. Second item\nits continuation'])

    def test_inline_fragments_join_but_column_gutter_does_not(self):
        blocks = group_lines([line('OCR test', 12, 23, 155, 41), line('123', 165, 24, 70, 37),
                              line('separate column', 400, 23, 200, 41)])
        self.assertEqual([b['text'] for b in blocks], ['OCR test 123', 'separate column'])
        self.assertEqual(blocks[0]['bounds'], [12, 23, 223, 41])

    def test_overlapping_word_fragments_do_not_duplicate_boundary(self):
        blocks = group_lines([line('and s', 0, 2, 67), line('stays', 55, 0, 75), line('here', 138, 1, 60)])
        self.assertEqual([b['text'] for b in blocks], ['and stays here'])

    def test_slight_bbox_overlap_keeps_intentional_repeated_words(self):
        blocks = group_lines([line('very', 0, 0, 50), line('very', 48, 1, 50)])
        self.assertEqual([b['text'] for b in blocks], ['very very'])

    def test_small_bbox_overlap_and_jitter_still_form_paragraph(self):
        blocks = group_lines([line('first', 10, 0, 300), line('second', 12, 18, 300), line('third', 9, 42, 300)])
        self.assertEqual(len(blocks), 1)

    def test_normal_paragraph_over_600_chars_is_not_arbitrarily_split(self):
        rows = [line('中' * 200, 0, index * 30, 400) for index in range(5)]
        blocks = group_lines(rows)
        self.assertEqual(len(blocks), 1)
        self.assertEqual(blocks[0]['text'], '\n'.join(r['text'] for r in rows))

    def test_oversized_paragraph_splits_at_rows_with_matching_bounds(self):
        blocks = group_lines([line('中' * 1000, 0, y, 400) for y in (0, 30, 60)])
        self.assertEqual([b['bounds'] for b in blocks], [[0, 0, 400, 50], [0, 60, 400, 20]])

    def test_same_size_short_heading_stays_separate(self):
        blocks = group_lines([line('标题', 0, 0, 60), line('较长的正文第一行', 0, 30, 400), line('正文的下一行', 0, 60, 400)])
        self.assertEqual(len(blocks), 2)

    def test_empty_input(self):
        self.assertEqual(group_lines([]), [])
        self.assertEqual(group_lines([line('   ', 0, 0)]), [])

    def test_crop_never_reads_outside_image(self):
        self.assertEqual(validate_crop([20, 30, 80, 70], 100, 100), (20, 30, 100, 100))
        for rect in ([-1, 0, 20, 20], [0, 0, 101, 20], [0, 0, 2, 2], [0, 0, 20.5, 20]):
            with self.assertRaises(ValueError):
                validate_crop(rect, 100, 100)


class ProtocolTests(unittest.TestCase):
    def test_utf8_paths_and_text_survive_windows_legacy_encoding(self):
        class EchoWorker:
            def handle(self, request):
                return {'path': request['model_path'], 'text': request['text']}

        request = {'command': 'test', 'model_path': 'C:/用户/个人兴趣/模型🙂', 'text': '日本語・中文'}
        raw = (json.dumps(request, ensure_ascii=False) + '\n').encode('utf-8')
        # Mirrors a legacy Windows text wrapper; main uses its binary buffer.
        legacy_stdin = io.TextIOWrapper(io.BytesIO(raw), encoding='gbk')
        output = io.BytesIO()
        serve(EchoWorker(), legacy_stdin.buffer, output)
        response = json.loads(output.getvalue())
        self.assertTrue(response['ok'])
        self.assertEqual(response['path'], request['model_path'])
        self.assertEqual(response['text'], request['text'])

    def test_bad_request_does_not_desynchronize_next_request(self):
        class EchoWorker:
            def handle(self, request):
                return {'protocol': 1}

        output = io.BytesIO()
        serve(EchoWorker(), io.BytesIO(b'\xff\n{"command":"ping"}\n{"command":"shutdown"}\n'), output)
        responses = [json.loads(row) for row in output.getvalue().splitlines()]
        self.assertEqual([row['ok'] for row in responses], [False, True])


if __name__ == '__main__':
    unittest.main()
