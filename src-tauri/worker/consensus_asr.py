"""Causal three-window agreement. No LLM arbitration or reference transcript.

Four-second ownership units select hypotheses; twelve-second display groups
are translated/replaced atomically so a word crossing an ownership boundary
is never sent to the translator as two separate fragments.
"""
from dataclasses import dataclass, field
import math
import time
import numpy as np
from streaming_asr import RATE, Window, normalized


def norm(text):
    return normalized(text)[0]


def mid(word):
    return (word['a'] + word['b']) / 2


def after_anchor(previous, words):
    if not previous:
        return words
    past = norm(''.join(w['t'] for w in previous))
    current, ends = '', []
    for i, word in enumerate(words):
        current += norm(word['t'])
        ends.append((len(current), i, word['b']))
    boundary = previous[-1]['b']
    for size in range(min(24, len(past)), 2, -1):
        matches = [i for end, i, stamp in ends
                   if current[max(0, end-size):end] == past[-size:]
                   and abs(stamp-boundary) <= 1.5]
        if len(matches) == 1:
            return words[matches[0]+1:]
    return [w for w in words if mid(w) > boundary + .02]


def chars(words):
    result = []
    for word in words:
        value, offsets = normalized(word['t'])
        for i, char in enumerate(value):
            # Equality ignores punctuation; output retains original spelling,
            # spaces and punctuation, including NFKC expansions only once.
            a = 0 if i == 0 else offsets[i]
            b = offsets[i+1] if i+1 < len(offsets) else len(word['t'])
            result.append(dict(c=char, t=mid(word), raw=word['t'][a:b]))
    return result


def align(base, new):
    n, m = len(base), len(new)
    dp = np.zeros((n+1, m+1))
    op = np.zeros((n+1, m+1), dtype=np.uint8)
    dp[:, 0] = np.arange(n+1)
    dp[0, :] = np.arange(m+1)
    op[1:, 0], op[0, 1:] = 1, 2
    for i in range(1, n+1):
        for j in range(1, m+1):
            dt = abs(base[i-1]['t'] - new[j-1]['t'])
            sub = min(dt*.08, 2.1) if base[i-1]['c'] == new[j-1]['c'] else 1+min(dt*.08, .8)
            costs = (dp[i-1, j-1]+sub, dp[i-1, j]+1, dp[i, j-1]+1)
            k = min(range(3), key=lambda k: costs[k])
            dp[i, j], op[i, j] = costs[k], k
    pairs, i, j = [], n, m
    while i or j:
        k = op[i, j]
        if k == 0:
            pairs.append((i-1, j-1)); i -= 1; j -= 1
        elif k == 1:
            pairs.append((i-1, None)); i -= 1
        else:
            pairs.append((None, j-1)); j -= 1
    pairs.reverse()
    owners = {}
    for k, (bi, nj) in enumerate(pairs):
        if nj is None:
            continue
        if bi is not None:
            owner = base[bi]['owner']
        else:
            left = next((base[a] for a, _ in reversed(pairs[:k]) if a is not None), None)
            right = next((base[a] for a, _ in pairs[k+1:] if a is not None), None)
            near = [x for x in (left, right) if x is not None]
            if not near:
                continue
            owner = min(near, key=lambda x: abs(x['t']-new[nj]['t']))['owner']
        owners[owner] = owners.get(owner, '') + new[nj]['raw']
    return owners


@dataclass
class Unit:
    observations: list = field(default_factory=list)
    selected: str = ''
    frozen: bool = False

    def observe(self, text):
        if self.frozen:
            return
        self.observations.append(text)
        keys = [norm(x) for x in self.observations]
        if len(keys) == 1:
            self.selected = text
        elif keys[0] == keys[1]:
            self.frozen = True
        elif len(keys) == 3:
            # Silence/omission must not erase already displayed speech.
            if keys[1] == keys[2] and keys[1]:
                self.selected = self.observations[1]
            self.frozen = True


class ConsensusWindows:
    def __init__(self):
        self.end = 0
        self.next = 8 * RATE
        self.last_emitted = 0
        self.audio = np.empty(0, np.float32)

    def feed(self, frame):
        frame = np.asarray(frame, dtype=np.float32)
        if frame.ndim == 2:
            frame = frame.mean(axis=1)
        result = []
        while len(frame):
            count = min(len(frame), self.next-self.end)
            self.audio = np.concatenate((self.audio, frame[:count]))[-12*RATE:]
            frame = frame[count:]
            self.end += count
            if self.end == self.next:
                result.append(self.snapshot())
                self.last_emitted = self.end
                self.next += 4*RATE
        return result

    def snapshot(self):
        return Window(self.end, self.end-len(self.audio), self.end, True, self.audio.copy())

    def tail(self):
        return self.snapshot() if self.end > self.last_emitted else None


class Consensus:
    def __init__(self):
        self.units, self.groups = {}, {}
        self.backbone, self.previous = [], []
        self.seen_end = 0

    def push(self, words, start, end):
        fresh = after_anchor(self.previous[-40:], words)
        for w in fresh:
            owner = min(int(mid(w)//4), 1) if not self.seen_end else math.ceil(end/4)-1
            owner = max(0, owner)
            self.backbone.extend(dict(x, owner=owner) for x in chars([w]))
        self.previous = (self.previous + fresh)[-80:]
        eligible = [x for x in self.backbone if start-.8 <= x['t'] <= end+.8]
        mapped = align(eligible, chars(words))
        for i in range(int(start//4), math.ceil(end/4)):
            # A short final window may observe the tail, but no repeated
            # observation of a partially covered older unit is counted.
            if i*4 < start:
                continue
            unit = self.units.setdefault(i, Unit())
            value = mapped.get(i, '') if unit.observations else ''.join(
                x['raw'] for x in self.backbone if x['owner'] == i)
            unit.observe(value)
        changes = []
        for group in sorted({i//3 for i in self.units}):
            text = ''.join(self.units[i].selected for i in range(group*3, group*3+3)
                           if i in self.units).strip()
            old, revision = self.groups.get(group, ('', 0))
            if norm(text) and norm(text) != norm(old):
                self.groups[group] = (text, revision+1)
                changes.append(dict(group_id=group, revision=revision+1, text=text,
                                    start_ms=group*12000, end_ms=round(min(end, (group+1)*12)*1000)))
        self.seen_end = end
        # Finished groups are immutable; bound all state independently of
        # stream length. Keep enough anchors for the next twelve-second window.
        expired = [g for g in self.groups if (g+1)*12 <= end-12]
        for g in expired:
            del self.groups[g]
            for i in range(g*3, g*3+3):
                self.units.pop(i, None)
        self.units = {i: u for i, u in self.units.items() if (i//3+1)*12 > end-12}
        self.backbone = [x for x in self.backbone if x['t'] >= end-16]
        return changes


class ConsensusRecognizer:
    def __init__(self, model, model_id, emit):
        self.model, self.model_id, self.emit = model, model_id, emit
        self.consensus = Consensus()

    def process(self, window):
        started = time.perf_counter()
        words = []
        if len(window.audio) >= RATE//5 and float(np.sqrt(np.mean(window.audio**2))) >= .001:
            segments, _ = self.model.transcribe(
                window.audio, language='ja', task='transcribe', beam_size=3,
                best_of=1, temperature=0, condition_on_previous_text=False,
                vad_filter=False, word_timestamps=True)
            for segment in segments:
                if (segment.avg_logprob < -1 or segment.compression_ratio > 2.4
                        or (segment.no_speech_prob > .8 and segment.avg_logprob < -.5)):
                    continue
                for word in segment.words or []:
                    words.append(dict(a=window.start/RATE+word.start,
                                      b=window.start/RATE+word.end, t=word.word))
        for update in self.consensus.push(words, window.start/RATE, window.end/RATE):
            self.emit('review', **update, model_id=self.model_id,
                      latency_ms=round((time.perf_counter()-started)*1000))

    def flush(self):
        # Unconfirmed EOF hypotheses already display their first result.
        # No artificial silence uploads or extra recognition passes.
        pass
