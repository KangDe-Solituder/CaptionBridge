"""CPU OCR worker. NDJSON over stdio; screenshots stay in memory, never in logs."""
import base64
import ctypes
import io
import json
import os
from pathlib import Path
import sys
import time
from contextlib import redirect_stdout

MAX_PIXELS = 24_000_000
PROTOCOL = sys.stdout


def group_lines(lines):
    """Conservative paragraph grouping: adjacent, left-aligned lines only.

    Keep disconnected columns separate and retain crop-relative bounding boxes.
    Long paragraphs are split so each translation has a bounded input.
    """
    groups = []
    for line in sorted(lines, key=lambda x: (x['bounds'][1], x['bounds'][0])):
        x, y, w, h = line['bounds']
        text = line['text'].strip()
        if not text:
            continue
        candidate = None
        for g in reversed(groups):
            lx, ly, lw, lh = g['_last']
            gap = y - (ly + lh)
            if (0 <= gap <= max(h, lh) * 0.85
                    and abs(x - lx) <= max(h, lh) * 0.7
                    and min(w, lw) > 0
                    and max(h, lh) / max(1, min(h, lh)) < 1.5
                    and len(g['text']) + len(text) < 600):
                candidate = g
                break
        if candidate is None:
            groups.append({'text': text, 'bounds': list(line['bounds']),
                           'confidence': line['confidence'], '_last': line['bounds']})
        else:
            gx, gy, gw, gh = candidate['bounds']
            candidate['text'] += '\n' + text
            candidate['bounds'] = [min(gx, x), min(gy, y),
                                   max(gx + gw, x + w) - min(gx, x),
                                   max(gy + gh, y + h) - min(gy, y)]
            candidate['confidence'] = min(candidate['confidence'], line['confidence'])
            candidate['_last'] = line['bounds']
    # A block is always a separately editable/copyable item, never a flattened page.
    result = []
    for g in sorted(groups, key=lambda x: (x['bounds'][1], x['bounds'][0])):
        g.pop('_last')
        text = g['text']
        for start in range(0, len(text), 600):
            result.append({**g, 'id': len(result) + 1, 'text': text[start:start + 600]})
    return result


def validate_crop(rect, width, height):
    if len(rect) != 4 or any(type(v) is not int for v in rect):
        raise ValueError('选区坐标无效')
    x, y, w, h = rect
    if x < 0 or y < 0 or w < 8 or h < 8 or x + w > width or y + h > height:
        raise ValueError('选区超出截图边界，或选区太小')
    return x, y, x + w, y + h


class Worker:
    def __init__(self):
        self.engine = None
        self.model_path = None

    def load(self, model_path):
        model_path = str(Path(model_path).resolve())
        if self.engine is not None and self.model_path == model_path:
            return 0
        for name in ('det.onnx', 'rec.onnx', 'cls.onnx'):
            if not (Path(model_path) / name).is_file():
                raise ValueError('OCR 模型未完整安装，请在设置 → 资源中下载或校验')
        start = time.perf_counter()
        from rapidocr import RapidOCR, EngineType, OCRVersion, ModelType
        params = {'Global.log_level': 'error', 'EngineConfig.onnxruntime.intra_op_num_threads': 2,
                  'EngineConfig.onnxruntime.inter_op_num_threads': 1}
        for stage, name in [('Det', 'det'), ('Rec', 'rec'), ('Cls', 'cls')]:
            params.update({f'{stage}.model_path': str(Path(model_path) / (name + '.onnx')),
                           f'{stage}.engine_type': EngineType.ONNXRUNTIME,
                           f'{stage}.ocr_version': OCRVersion.PPOCRV5,
                           f'{stage}.model_type': ModelType.MOBILE})
        self.engine = RapidOCR(params=params)
        self.model_path = model_path
        return round((time.perf_counter() - start) * 1000)

    def handle(self, request):
        from PIL import Image, ImageGrab
        command = request['command']
        if command == 'ping':
            return {'protocol': 1}
        if command == 'capture':
            left, top, width, height = request['monitor']
            if width < 1 or height < 1 or width * height > MAX_PIXELS:
                raise ValueError('屏幕分辨率超出截图支持范围')
            image = ImageGrab.grab(bbox=(left, top, left + width, top + height),
                                   include_layered_windows=True, all_screens=True)
            buf = io.BytesIO()
            image.save(buf, format='PNG')
            return {'image': base64.b64encode(buf.getvalue()).decode('ascii'),
                    'width': image.width, 'height': image.height}
        if command in ('recognize', 'test'):
            load_ms = self.load(request['model_path'])
            if command == 'test':
                from PIL import ImageDraw
                image = Image.new('RGB', (420, 90), 'white')
                ImageDraw.Draw(image).text((16, 20), 'OCR test 123', fill='black', font_size=36)
            else:
                image = Image.open(io.BytesIO(base64.b64decode(request['image'], validate=True)))
                if image.width * image.height > MAX_PIXELS:
                    raise ValueError('截图过大')
                image = image.crop(validate_crop(request['rect'], image.width, image.height)).convert('RGB')
            import numpy as np
            start = time.perf_counter()
            output = self.engine(np.array(image)[:, :, ::-1].copy())
            lines = []
            if output.boxes is not None and output.txts is not None:
                for box, text, score in zip(output.boxes, output.txts, output.scores):
                    xs, ys = box[:, 0], box[:, 1]
                    lines.append({'text': text, 'confidence': float(score),
                                  'bounds': [max(0, round(float(xs.min()))), max(0, round(float(ys.min()))),
                                             round(float(xs.max() - xs.min())), round(float(ys.max() - ys.min()))]})
            return {'blocks': group_lines(lines), 'load_ms': load_ms,
                    'recognize_ms': round((time.perf_counter() - start) * 1000)}
        raise ValueError('未知 OCR 命令')


def main():
    if os.name == 'nt':
        try:
            ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
        except (AttributeError, OSError):
            pass
    worker = Worker()
    for raw in sys.stdin:
        try:
            if len(raw) > 64 * 1024 * 1024:
                raise ValueError('请求过大')
            request = json.loads(raw)
            if request.get('command') == 'shutdown':
                break
            with redirect_stdout(sys.stderr):
                result = worker.handle(request)
            response = {'ok': True, **result}
        except Exception as error:
            response = {'ok': False, 'error': str(error)}
        PROTOCOL.write(json.dumps(response, ensure_ascii=True) + '\n')
        PROTOCOL.flush()


if __name__ == '__main__':
    main()
