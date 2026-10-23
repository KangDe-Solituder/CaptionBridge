"""Opt-in real inference, using OCR_TEST_MODEL_DIR and optionally OCR_TEST_WORKER.

Default unit tests need neither models nor OCR dependencies. The subprocess uses
raw UTF-8 requests (as Rust does), even when Windows selects a legacy codepage.
"""
import base64
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(os.environ.get('OCR_TEST_MODEL_DIR'), 'requires downloaded OCR models')
class InferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='ocr-integration-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.models = Path(cls.temp.name) / '中文模型_日本語_🙂'
        cls.models.mkdir()
        for name in ('det.onnx', 'rec.onnx', 'cls.onnx'):
            shutil.copyfile(Path(os.environ['OCR_TEST_MODEL_DIR']) / name, cls.models / name)

    def request(self, **request):
        request['model_path'] = str(self.models)
        exe = os.environ.get('OCR_TEST_WORKER')
        command = [exe] if exe else [sys.executable, str(Path(__file__).with_name('ocr_worker.py'))]
        result = subprocess.run(command, input=(json.dumps(request, ensure_ascii=False) + '\n').encode('utf-8'),
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90,
                                env={**os.environ, 'PYTHONIOENCODING': 'gbk', 'PYTHONUTF8': '0'})
        self.assertEqual(result.returncode, 0, result.stderr.decode('utf-8', errors='replace')[-2000:])
        response = json.loads(result.stdout)
        self.assertTrue(response['ok'], response)
        return response

    def recognize(self, image):
        buffer = io.BytesIO()
        image.save(buffer, format='PNG')
        return self.request(command='recognize', image=base64.b64encode(buffer.getvalue()).decode('ascii'),
                            rect=[0, 0, image.width, image.height])['blocks']

    def test_unicode_model_path_and_actual_inference(self):
        blocks = self.request(command='test')['blocks']
        self.assertEqual(len(blocks), 1)
        self.assertIn('OCR test 123', blocks[0]['text'])

    @unittest.skipUnless(os.name == 'nt', 'fixture uses Windows Chinese font')
    def test_chinese_wrapping_indent_and_paragraph_break(self):
        from PIL import Image, ImageDraw, ImageFont
        font = ImageFont.truetype(str(Path(os.environ['WINDIR']) / 'Fonts/msyh.ttc'), 28)
        image = Image.new('RGB', (1100, 380), 'white')
        draw = ImageDraw.Draw(image)
        for x, y, text in [
            (84, 24, '这是一段用于验证截图识别的中文内容，网页中的文字'),
            (28, 70, '经常因为窗口宽度自动换行，但翻译时应该保持段落完整，'),
            (28, 116, '而不是每一行单独翻译。'),
            (84, 210, '这是另外一段独立的内容，两段之间留出了明显的空白，'),
            (28, 256, '因此识别后应该保留段落边界。'),
        ]:
            draw.text((x, y), text, font=font, fill='black')
        blocks = self.recognize(image)
        self.assertEqual(len(blocks), 2, blocks)
        self.assertEqual([b['text'].count('\n') for b in blocks], [2, 1], blocks)

    @unittest.skipUnless(os.name == 'nt', 'fixture uses Windows font')
    def test_two_columns_do_not_interleave(self):
        from PIL import Image, ImageDraw, ImageFont
        font = ImageFont.truetype(str(Path(os.environ['WINDIR']) / 'Fonts/arial.ttf'), 24)
        image = Image.new('RGB', (1200, 230), 'white')
        draw = ImageDraw.Draw(image)
        for x, lines in [(24, ['The left column starts here', 'and continues on the next line', 'with one complete paragraph.']),
                         (650, ['The right column starts here', 'and stays separate from the left', 'with another complete paragraph.'])]:
            for index, text in enumerate(lines):
                draw.text((x, 24 + index * 40), text, font=font, fill='black')
        blocks = self.recognize(image)
        self.assertEqual(len(blocks), 2, blocks)
        self.assertIn('left column', blocks[0]['text'])
        self.assertNotIn('right column', blocks[0]['text'])
        self.assertIn('right column', blocks[1]['text'])
        self.assertEqual([b['text'].count('\n') for b in blocks], [2, 2], blocks)


if __name__ == '__main__':
    unittest.main()
