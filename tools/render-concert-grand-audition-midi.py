"""MIDI maquette from four authenticated physical audition WAVs.

This is a sampler approximation, NOT the full nonlinear physical MIDI engine.
Nearest root transposition, one recorded model intensity, smooth sustain loop,
measured scalar decay/release and binary MIDI pedals. No reference-library
samples, EQ, compressor or instrument replacement. Reports only in the repo.
"""
import argparse
from collections import Counter, defaultdict, deque
from dataclasses import dataclass
from fractions import Fraction
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import time

import mido
import numpy as np
from scipy.signal import correlate, firwin, resample_poly
import soundfile as sf

RATE = 48000
LABELS = {60: 'C4', 65: 'F4', 67: 'G4', 72: 'C5'}


@lru_cache(maxsize=128)
def pitch_conversion(semitones):
    fraction = Fraction(2**(-semitones/12)).limit_denominator(4096)
    up, down = fraction.numerator, fraction.denominator
    size = max(up, down)
    taps = None if size == 1 else firwin(20*size+1, 1/size, window=('kaiser', 9.))
    return up, down, taps


def transpose_bandlimited(source, semitones):
    up, down, taps = pitch_conversion(semitones)
    if up == down:
        return source.copy()
    return resample_poly(source, up, down, axis=0, window=taps)


def digest(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


@dataclass
class Strike:
    start: float
    note: int
    velocity: int
    channel: int
    soft: int
    key_down: bool = True
    sostenuto: bool = False
    release: float | None = None


def performances(events, end):
    """Pair repeated keys FIFO and retain key-up voices until both pedals lift."""
    strikes = []
    keys = defaultdict(deque)
    sustain = defaultdict(bool)
    sostenuto = defaultdict(bool)
    soft = defaultdict(int)
    counts = Counter()
    for at, msg in events:
        if msg.is_meta:
            continue
        counts[msg.type] += 1
        ch = msg.channel
        if msg.type == 'note_on' and msg.velocity:
            strike = Strike(at, msg.note, msg.velocity, ch, soft[ch])
            strikes.append(strike)
            keys[ch, msg.note].append(strike)
        elif msg.type == 'note_off' or msg.type == 'note_on':
            if not keys[ch, msg.note]:
                raise ValueError(f'unpaired note off: {at}, {msg}')
            strike = keys[ch, msg.note].popleft()
            strike.key_down = False
            if not sustain[ch] and not strike.sostenuto:
                strike.release = at
        elif msg.type == 'control_change':
            if msg.control == 64:
                sustain[ch] = msg.value >= 64
            elif msg.control == 66:
                now = msg.value >= 64
                if now and not sostenuto[ch]:
                    for strike in strikes:
                        if strike.channel == ch and strike.key_down:
                            strike.sostenuto = True
                elif not now:
                    for strike in strikes:
                        if strike.channel == ch:
                            strike.sostenuto = False
                sostenuto[ch] = now
            elif msg.control == 67:
                soft[ch] = msg.value
            elif msg.control in (120, 123):
                for (channel, _), held in keys.items():
                    if channel == ch:
                        for strike in held:
                            strike.key_down = False
                        held.clear()
                if msg.control == 120:
                    sustain[ch] = False
                    for strike in strikes:
                        if strike.channel == ch:
                            strike.sostenuto = False
            elif msg.control == 121:
                sustain[ch] = sostenuto[ch] = False
                soft[ch] = 0
                for strike in strikes:
                    if strike.channel == ch:
                        strike.sostenuto = False
            for strike in strikes:
                if (strike.channel == ch and not strike.key_down
                        and strike.release is None and not sustain[ch] and not strike.sostenuto):
                    strike.release = at
        elif msg.type == 'pitchwheel':
            if msg.pitch != 0:
                raise ValueError('nonzero pitch wheel requires an additional sampler path')
        elif msg.type not in ('program_change', 'aftertouch', 'polytouch'):
            raise ValueError(f'unsupported channel message: {msg}')
    if any(keys.values()):
        raise ValueError('MIDI ends with unmatched held keys')
    for strike in strikes:
        if strike.release is None:
            strike.release = end
        if strike.release < strike.start:
            raise ValueError('negative gate duration')
    return strikes, dict(counts)


def load_performance(path):
    seconds = 0.
    events = []
    controllers = defaultdict(Counter)
    for msg in mido.MidiFile(path):
        seconds += msg.time  # mido applies the original tempo map.
        events.append((seconds, msg))
        if msg.type == 'control_change':
            controllers[msg.control][msg.value] += 1
    strikes, counts = performances(events, seconds)
    return strikes, seconds, dict(message_counts=counts,
        controller_values={str(k):dict(v) for k, v in controllers.items()},
        channel_events=sum(counts.values()), midi_sha256=digest(path))


class Anchor:
    def __init__(self, root, path, expected_hash):
        if digest(path) != expected_hash:
            raise ValueError('changed physical audition WAV')
        audio, rate = sf.read(path, dtype='float64', always_2d=True)
        if rate != RATE or audio.shape[1] != 2 or not np.isfinite(audio).all():
            raise ValueError('anchor format')
        # Remove exactly the known 100ms silent lead. Keep propagation delay.
        audio = audio[4800:]
        times = np.arange(.08, .321, .02)
        rms = np.array([np.sqrt(np.mean(audio[round(t*RATE):round((t+.02)*RATE)]**2))
                        for t in times])
        gamma = -float(np.polyfit(times+.01, np.log(rms), 1)[0])
        if gamma <= 0 or not np.isfinite(gamma):
            raise ValueError('positive sustained decay cannot be inferred')
        intercept = float(np.mean(np.log(rms)+gamma*(times+.01)))
        self.root, self.gamma = root, gamma
        # Undo one measured exponential for the periodic template, then put
        # the same decay back on the whole voice. Original prefix is retained.
        b = round(.33*RATE)
        fade = round(.03*RATE)
        template = audio[:b]*np.exp(gamma*np.arange(b)/RATE)[:, None]
        tail = template[b-fade:b]
        low, high = round(.09*RATE), round(.25*RATE)
        dot = sum(correlate(template[low:high+fade, c], tail[:, c],
                            mode='valid', method='fft') for c in range(2))
        energy = np.sum(template[low:high+fade]**2, axis=1)
        sums = np.r_[0., np.cumsum(energy)]
        norms = sums[fade:]-sums[:-fade]
        scores = dot/np.sqrt(np.maximum(norms*np.sum(tail*tail), 1e-30))
        a = low+int(np.argmax(scores))
        w = np.sin(np.linspace(0, np.pi/2, fade))**2
        template[b-fade:b] = tail*(1-w[:, None])+template[a:a+fade]*w[:, None]
        self.template = template
        self.loop_start = a+fade
        self.period = b-self.loop_start
        self.indices = np.arange(b)
        # Scalar note-off envelope estimated from the physical damper trace.
        tau = np.arange(.0, .641, .01)
        release_rms = np.array([np.sqrt(np.mean(audio[round((.35+t)*RATE):
            round((.35+t+.01)*RATE)]**2)) for t in tau])
        expected = np.exp(intercept-gamma*(.35+tau+.005))
        values = np.minimum.accumulate(np.clip(release_rms/expected, 0, 1))
        self.release_time = np.r_[0., tau+.005, .66]
        self.release_gain = np.r_[1., values, 0.]
        self.metadata = dict(root=root, source=str(path.resolve()), source_sha256=expected_hash,
            model_velocity=84, gamma_per_s=gamma, loop_start=self.loop_start,
            loop_end=b-1, loop_crossfade_frames=fade,
            loop_matching_correlation=float(scores.max()),
            release_time_s=self.release_time.tolist(), release_gain=self.release_gain.tolist())

    def voice(self, strike):
        offset = strike.note-self.root
        up, down, _ = pitch_conversion(offset)
        ratio = down/up
        release_s = .66/ratio
        duration = strike.release-strike.start+release_s
        count = max(1, round(duration*RATE))
        native_count = int(np.ceil(count*ratio))
        position = np.arange(native_count)
        clock = position/RATE
        mapped = np.where(position < len(self.template), position,
            self.loop_start+np.mod(position-self.loop_start, self.period))
        output = self.template[mapped]
        level = (strike.velocity/84)**1.5 * 10**(-3*strike.soft/127/20)
        envelope = np.exp(-self.gamma*clock)*level
        after = clock-(strike.release-strike.start)*ratio
        envelope *= np.interp(after, self.release_time, self.release_gain, left=1., right=0.)
        return transpose_bandlimited(output*envelope[:, None], offset)[:count], ratio


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--physical-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--replace', action='store_true', help='Replace the two named music WAVs only.')
    parser.add_argument('midi', type=Path, nargs='+')
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve the previous report')
    begun = time.perf_counter()
    source = json.loads(args.physical_report.read_text(encoding='utf-8'))
    if source['stage'] != 'audible current physical pilot completed':
        raise ValueError('completed physical source report required')
    sources = {Path(item['path']).stem:item for item in source['audio_files']}
    anchors = {}
    for root, name in LABELS.items():
        item = sources[name+'-84']
        anchors[root] = Anchor(root, Path(item['path']), item['sha256'])
    report = dict(render_kind='sampled musical maquette, NOT full physical simulation',
        all_gates_passed=False, script_sha256=digest(Path(__file__)),
        physical_source_report=str(args.physical_report.resolve()),
        physical_source_report_sha256=digest(args.physical_report),
        reference_library_samples_used=False, anchors=[a.metadata for a in anchors.values()],
        limitations=[
            'Only four physical root notes at velocity 84; all other keys transposed, including large octave extrapolations.',
            'Velocity changes gain by (v/84)^1.5; velocity-dependent felt spectra are NOT simulated.',
            'Sustain extrapolates one scalar measured decay with a phase-matched smooth loop.',
            'Release uses one scalar measured damper envelope, with pitch-scaled duration.',
            'Independent sampled voices; no shared board, sympathetic interaction, unison or repeated-hammer dynamics.',
            'CC64/66 thresholds at 64, all original values counted; no identified physical half pedal.',
            'CC67 changes new attacks by up to -3dB; not a physical una-corda mechanism.',
            'Program changes and aftertouch retained in counts; this one-voicing sampler has no response to them.'],
        sample_rate=RATE, channels=2, pieces=[])
    jobs = []
    for path in args.midi:
        strikes, seconds, info = load_performance(path)
        audio = np.zeros((round((seconds+10)*RATE), 2), np.float32)
        mapping = Counter()
        for index, strike in enumerate(strikes):
            root = min(anchors, key=lambda n:(abs(strike.note-n), n))
            voice, _ = anchors[root].voice(strike)
            start = round(strike.start*RATE)
            count = min(len(voice), len(audio)-start)
            audio[start:start+count] += voice[:count].astype(np.float32)
            mapping[strike.note, root] += 1
            if (index+1)%500 == 0:
                print(path.name, 'attacks', index+1, '/', len(strikes), flush=True)
        if not np.isfinite(audio).all() or np.max(abs(audio)) <= 0:
            raise ValueError('invalid musical waveform')
        info.update(midi=str(path.resolve()), midi_seconds=seconds, notes=len(strikes),
            note_range=[min(s.note for s in strikes), max(s.note for s in strikes)],
            velocity_range=[min(s.velocity for s in strikes), max(s.velocity for s in strikes)],
            exact_root_attacks=sum(count for (note, root), count in mapping.items() if note == root),
            transposed_attacks=sum(count for (note, root), count in mapping.items() if note != root),
            max_transposition_semitones=max(abs(note-root) for note, root in mapping),
            mapping=[dict(note=note, root=root, attacks=count) for (note, root), count in sorted(mapping.items())],
            frames=len(audio), raw_peak=float(np.max(abs(audio))))
        jobs.append((path, audio, info))
    gain = min(1., .85/max(item[2]['raw_peak'] for item in jobs))
    report['common_mix_gain'] = gain
    args.output.mkdir(parents=True, exist_ok=True)
    for path, audio, info in jobs:
        name = 'Maqueta-La-Campanella.wav' if 'campanella' in path.stem else 'Maqueta-Nocturno.wav'
        destination = args.output/name
        if destination.exists() and not args.replace:
            raise ValueError('preserve existing music export')
        audio *= gain
        sf.write(destination, audio, RATE, subtype='PCM_24')
        info.update(output=str(destination.resolve()), wav_sha256=digest(destination),
            output_peak=float(np.max(abs(audio))), common_mix_gain=gain)
        report['pieces'].append(info)
        print('WROTE', destination, 'peak', info['output_peak'], flush=True)
    report['seconds'] = time.perf_counter()-begun
    offsets = sorted({row['note']-row['root'] for job in report['pieces'] for row in job['mapping']})
    report['pitch_conversion'] = dict(method='Kaiser-9 polyphase anti-alias FIR; rational rate factors, denominator <=4096',
        max_ratio_error_cents=max(abs(1200*np.log2((pitch_conversion(d)[1]/pitch_conversion(d)[0]) / 2**(d/12))) for d in offsets))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, allow_nan=False), encoding='utf-8')


if __name__ == '__main__':
    main()
