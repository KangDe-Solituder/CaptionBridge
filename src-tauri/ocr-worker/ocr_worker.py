"""CPU OCR worker. NDJSON over stdio; screenshots stay in memory, never in logs."""
import base64
import ctypes
import io
import json
import os
from pathlib import Path
import re
from statistics import median
import sys
import time
from contextlib import redirect_stdout

MAX_PIXELS = 24_000_000
PROTOCOL = sys.stdout
MAX_BLOCK_CHARS = 2400
LIST_START = re.compile(r'^(?:[•●○▪■◆◇※]|[-–—]\s|\d{1,3}[.)、]\s*|[（(][\d一二三四五六七八九十百]+[)）]|[一二三四五六七八九十百]+[、．])')


def union_bounds(a, b):
    x, y = min(a[0], b[0]), min(a[1], b[1])
    return [x, y, max(a[0] + a[2], b[0] + b[2]) - x,
            max(a[1] + a[3], b[1] + b[3]) - y]


def text_rows(lines):
    """Join nearby fragments on the same baseline, without spanning wide gutters."""
    bands = []
    for line in sorted(lines, key=lambda x: (x['bounds'][1], x['bounds'][0])):
        x, y, w, h = line['bounds']
        text = line['text'].strip()
        if not text or w <= 0 or h <= 0:
            continue
        candidate = None
        for band in reversed(bands):
            row = band[0]
            rx, ry, rw, rh = row['bounds']
            height = max(h, rh)
            if (abs(y + h / 2 - ry - rh / 2) <= height * .35
                    and min(y + h, ry + rh) - max(y, ry) >= min(h, rh) * .6
                    and height / min(h, rh) < 1.5):
                candidate = band
                break
        row = {'text': text, 'bounds': list(line['bounds']), 'confidence': line['confidence']}
        if candidate is None:
            bands.append([row])
        else:
            candidate.append(row)
    rows = []
    for band in bands:
        current = None
        for row in sorted(band, key=lambda row: row['bounds'][0]):
            x, y, w, h = row['bounds']
            gap = x - current['bounds'][0] - current['bounds'][2] if current else float('inf')
            if current is None or gap > max(h, current['bounds'][3]) * .9:
                current = row
                rows.append(current)
                continue
            left, right = current['text'], row['text']
            overlap = 0
            if gap < 0:
                # Detection boxes sometimes overlap at a word boundary ("and s"
                # + "stays"). Remove only matching text in that overlap.
                # Slight box overlap must not erase an intentional repeated word.
                overlap_chars = round(-gap / (w / len(right)))
                for size in range(1, min(len(left), len(right), overlap_chars) + 1):
                    if left.endswith(right[:size]):
                        overlap = size
            cjk = lambda c: '\u2e80' <= c <= '\ua4cf' or '\uac00' <= c <= '\ud7af' or '\uf900' <= c <= '\ufaff'
            separator = '' if overlap or (cjk(left[-1]) and cjk(right[0])) else ' '
            current['text'] = left + separator + right[overlap:]
            current['bounds'] = union_bounds(current['bounds'], row['bounds'])
            current['confidence'] = min(current['confidence'], row['confidence'])
    return sorted(rows, key=lambda row: (row['bounds'][1], row['bounds'][0]))


def reading_order(groups):
    """Read separate columns top-to-bottom, after any full-width heading."""
    if len(groups) < 2:
        return groups
    height = median(row['bounds'][3] for group in groups for row in group)
    rectangles = []
    for group in groups:
        bounds = group[0]['bounds']
        for row in group[1:]:
            bounds = union_bounds(bounds, row['bounds'])
        rectangles.append((bounds, group))
    region_width = max(bounds[0] + bounds[2] for bounds, _ in rectangles) - min(bounds[0] for bounds, _ in rectangles)
    for bounds, group in rectangles:
        if bounds[2] < region_width * .8:
            continue
        above = [other for box, other in rectangles if other is not group and box[1] + box[3] <= bounds[1]]
        below = [other for box, other in rectangles if other is not group and box[1] >= bounds[1] + bounds[3]]
        if len(above) + len(below) == len(groups) - 1:
            return reading_order(above) + [group] + reading_order(below)
    # Whitespace cuts: vertical gutters first; horizontal gaps isolate a heading
    # spanning multiple columns, so the remaining region can be split again.
    for axis, minimum_gap in ((0, height), (1, height * .5)):
        ordered = sorted(rectangles, key=lambda item: item[0][axis])
        partitions, current = [], []
        end = float('-inf')
        for bounds, group in ordered:
            if current and bounds[axis] - end >= minimum_gap:
                partitions.append(current)
                current = []
            current.append(group)
            end = max(end, bounds[axis] + bounds[axis + 2])
        partitions.append(current)
        if len(partitions) > 1:
            return [group for part in partitions for group in reading_order(part)]
    # Near-identical top edges can differ by a few OCR pixels.
    return sorted(groups, key=lambda group: (round(group[0]['bounds'][1] / height), group[0]['bounds'][0]))


def paragraph_distance(group, row):
    """Score a continuation using spacing, overlap, indentation and typography."""
    previous = group[-1]
    x, y, w, h = row['bounds']
    lx, ly, lw, lh = previous['bounds']
    height = max(h, lh)
    gap = y - ly - lh
    overlap = min(x + w, lx + lw) - max(x, lx)
    if (not -height * .15 <= gap <= height * 1.6
            or height / min(h, lh) >= 1.5
            or overlap < min(w, lw) * .6
            or LIST_START.match(row['text'])):
        return None
    # After the first line, the body margin is stable. A new positive indent
    # starts a paragraph; a first-line indent or list hanging indent can unwind.
    margin = median([r['bounds'][0] for r in (group[1:] or group)])
    offset = x - margin
    first_indent = len(group) == 1 and -2.5 * height <= offset <= 0
    hanging = len(group) == 1 and LIST_START.match(previous['text']) and 0 <= offset <= 2.5 * height
    if abs(offset) > height * .75 and not (first_indent or hanging):
        return None
    if len(group) > 1:
        gaps = [max(0, b['bounds'][1] - a['bounds'][1] - a['bounds'][3])
                for a, b in zip(group, group[1:])]
        if gap > median(gaps) + height * .65:
            return None
        full_width = max(r['bounds'][2] for r in group)
        if lw < full_width * .75 and (lw < full_width * .5 or re.search(r'[。！？.!?；;：:]\s*$', previous['text'])):
            return None
    elif lw < w * .5 and not LIST_START.match(previous['text']):
        # A short standalone heading above a wider body is not its first line.
        return None
    return max(0, gap) / height + abs(offset) / height * .2


def group_lines(lines):
    """Recover horizontal paragraphs while retaining line breaks in the source.

    Each paragraph is translated together. Columns, headings, blank lines,
    paragraph indents and separate list items remain independent blocks.
    """
    groups = []
    for row in text_rows(lines):
        candidates = [(score, index) for index, group in enumerate(groups)
                      if (score := paragraph_distance(group, row)) is not None]
        if candidates:
            groups[min(candidates)[1]].append(row)
        else:
            groups.append([row])
    result = []
    for group in reading_order(groups):
        block = None
        for row in group:
            text = row['text']
            # Split only oversized inputs. Normal wrapped paragraphs stay whole;
            # prefer row boundaries so each split retains an accurate rectangle.
            for start in range(0, len(text), MAX_BLOCK_CHARS):
                part = text[start:start + MAX_BLOCK_CHARS]
                if block is None or len(block['text']) + len(part) + 1 > MAX_BLOCK_CHARS:
                    block = {**row, 'bounds': list(row['bounds']), 'text': part, 'id': len(result) + 1}
                    result.append(block)
                else:
                    block['text'] += '\n' + part
                    block['bounds'] = union_bounds(block['bounds'], row['bounds'])
                    block['confidence'] = min(block['confidence'], row['confidence'])
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


def serve(worker, input_stream, output_stream):
    # NDJSON is UTF-8 bytes on both sides. Windows text-mode stdin may otherwise
    # decode Rust's UTF-8 paths as GBK/ANSI, making installed models look missing.
    for raw in input_stream:
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
        output_stream.write((json.dumps(response, ensure_ascii=True) + '\n').encode('utf-8'))
        output_stream.flush()


def main():
    if os.name == 'nt':
        try:
            ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
        except (AttributeError, OSError):
            pass
    serve(Worker(), sys.stdin.buffer, PROTOCOL.buffer)


if __name__ == '__main__':
    main()
