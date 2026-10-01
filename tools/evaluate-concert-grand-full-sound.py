"""Evaluate a frozen piano across all bitKlavier roots/layers, with one gain.

Recording comparisons and controlled SFZ releases are reported separately.
Metrics are audio diagnostics, not physical identification or fidelity scores.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import csv
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import stft

RATE = 44100
HOLD = 12
SECONDS = 16
ROOTS = tuple(range(21, 109, 3))
VELOCITIES = tuple(range(4, 128, 8))
STAGES = ((0, .02), (.02, .05), (.05, .2), (.2, .5), (.5, 1.5),
          (1.5, 3.5), (3.5, 7.5), (7.5, 11.5))
BANDS = ((25, 100), (100, 250), (250, 600), (600, 1500),
         (1500, 4000), (4000, 8000), (8000, 16000))
CALIBRATION = {(n, v) for n in (60, 66, 72, 78, 84, 90, 96) for v in (60, 124)}
MAPPED = {(n, v) for n in (65, 67, 79, 91) for v in (60, 84, 124)}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def db(power):
    return 10 * np.log10(np.maximum(power, 1e-18))


def power(x):
    return float(np.mean(np.asarray(x, dtype=np.float64)**2))


def onset(x, threshold=-40):
    # Both-channel energy avoids treating opposite stereo phases as silence.
    step = 44
    z = x[:min(len(x), RATE)//step*step].reshape(-1, step, 2)
    p = np.mean(z*z, axis=(1, 2))
    assert p.max() > 0
    at = np.flatnonzero(p > p.max()*10**(threshold/10))
    return int(at[0]*step)


def aligned(x):
    at = onset(x)
    return np.pad(x[at:], ((0, at), (0, 0))), at


def psd(x):
    size = max(32768, 1 << (len(x)-1).bit_length())
    window = np.hanning(len(x))
    z = np.fft.rfft(x*window[:, None], n=size, axis=0)
    p = 2*np.mean(abs(z)**2, axis=1)/(size*np.sum(window*window))
    p[0] *= .5
    p[-1] *= .5
    return np.fft.rfftfreq(size, 1/RATE), p


def features(x, note, centers=None):
    y, at = aligned(x)
    f0 = 440*2**((note-69)/12)
    if centers is None:
        hz, p = psd(y[round(.05*RATE):round(.5*RATE)])
        centers = []
        for h in range(1, 25):
            if h*f0 > 15000:
                break
            bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
            centers.append(float(hz[bins[np.argmax(p[bins])]]))
    rows = []
    for lo, hi in STAGES:
        clip = y[round(lo*RATE):round(hi*RATE)]
        hz, p = psd(clip)
        radii = max(8, .025*f0)
        partials = [float(p[abs(hz-f) < radii].sum()) for f in centers]
        mid = (clip[:, 0]+clip[:, 1])*.5
        side = (clip[:, 0]-clip[:, 1])*.5
        rows.append(dict(stage=[lo, hi], total_db=float(db(power(clip))),
            bands_db=[float(db(p[(hz >= low) & (hz < high)].sum())) for low, high in BANDS],
            subfundamental_db=float(db(p[hz < .7*f0].sum())),
            partials_db=[float(db(q)) for q in partials] if lo >= .05 else None,
            stereo_side_mid_db=float(db(power(side))-db(power(mid))),
            left_db=float(db(power(clip[:, 0]))), right_db=float(db(power(clip[:, 1])))))
    releases = []
    baseline = float(db(power(x[round(11.5*RATE):12*RATE])))
    for lo, hi in ((0, .05), (.05, .2), (.2, .5), (.5, 1), (1, 2), (2, 4)):
        z = x[round((HOLD+lo)*RATE):round((HOLD+hi)*RATE)]
        releases.append(dict(after_note_off=[lo, hi], level_db=float(db(power(z))),
                             relative_preoff_db=float(db(power(z)))-baseline))
    chunk = 441
    z = y[:len(y)//chunk*chunk].reshape(-1, chunk, 2)
    envelope = db(np.mean(z*z, axis=(1, 2)))
    return dict(onset_s=at/RATE,
        onset_threshold_s={str(t):onset(x, t)/RATE for t in (-50, -40, -30)},
        centers_hz=centers, stages=rows, release=releases,
        envelope_10ms_db=envelope.round(4).tolist(), peak=float(abs(x).max()))


def multi_spectrum(reference, model, support):
    """Fixed gain; union masks include excess model bands and missing target bands."""
    target, _ = aligned(reference)
    prediction, _ = aligned(model)
    edges = np.geomspace(25, 16000, 49)
    results = []
    for size, hop in ((512, 128), (2048, 512), (8192, 2048)):
        values = []
        for x in (target, prediction):
            hz, times, z = stft(x, RATE, nperseg=size, noverlap=size-hop,
                               boundary='zeros', padded=False, axis=0)
            p = np.mean(abs(z)**2, axis=1)
            bands = [p[(hz >= lo) & (hz < hi)].sum(axis=0)
                     for lo, hi in zip(edges[:-1], edges[1:])
                     if np.any((hz >= lo) & (hz < hi))]
            values.append(db(np.stack(bands, axis=1)))
        a, b = values
        # Reference-relative floor is only a reliability mask, not a level match.
        floor = a[(times >= .05) & (times < .3)].max()-60
        mask = (a > floor) | (b > floor)
        rows = []
        for lo, hi in STAGES:
            keep = mask & ((times >= lo) & (times < hi))[:, None]
            rows.append(float(np.mean(abs(a-b)[keep])) if hi <= support and keep.any() else None)
        results.append(dict(window_samples=size, hop_samples=hop, stage_mae_db=rows))
    return results


def render_model(binary, frozen, directory, note, velocity):
    directory.mkdir(parents=True, exist_ok=True)
    raw = directory/'performance.f32'
    signature = dict(binary_sha256=frozen['binary_sha256'], model=frozen, note=note,
                     velocity=velocity, hold=HOLD, seconds=SECONDS, rate=RATE)
    marker = directory/'manifest.json'
    expected = RATE*SECONDS*2*4
    if not (raw.exists() and raw.stat().st_size == expected and marker.exists()
            and json.loads(marker.read_text()) == signature):
        event = directory/'events.txt'
        event.write_text(f'0 144 {note} {velocity}\n{HOLD*RATE} 128 {note} 64\n')
        tuning = directory/'tuning.txt'
        tuning.write_text('\n'.join(f'{k} = {v}' for k, v in sorted(frozen['tuning'].items()))+'\n')
        env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event.resolve()), CG_TOTAL_FRAMES=str(RATE*SECONDS),
            CG_RATE=str(RATE), CG_TUNING=str(tuning.resolve()), CG_RENDER_DIR=str(directory.resolve()),
            CG_PRESET='concert', CG_PARAMS=','.join(f'{k}={v}' for k,v in frozen['params'].items()),
            RUST_MIN_STACK='33554432')
        with (directory/'render.log').open('w') as log:
            subprocess.run([str(binary.resolve()), '--exact', 'tests::render_reference_wavs',
                            '--ignored', '--quiet'], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        assert raw.stat().st_size == expected
        marker.write_text(json.dumps(signature, indent=2), encoding='utf-8')
    x = np.fromfile(raw, dtype='<f4').reshape(-1, 2)
    assert np.isfinite(x).all() and abs(x).max() > 0
    return raw


def read_model(path):
    return np.fromfile(path, dtype='<f4').reshape(-1, 2).astype(np.float64)


def render_reference(synth, note, velocity):
    synth.send_cc(synth.s, 0, 120, 0)
    for cc in (64, 66, 67):
        synth.send_cc(synth.s, 0, cc, 0)
    for _ in range(16):
        synth.render(1024)
    assert synth.get_num_active_voices(synth.s) == 0, 'Reference state was not cleared'
    synth.send_note_on(synth.s, 0, note, velocity)
    x = np.empty((RATE*SECONDS, 2), dtype=np.float64)
    for start in range(0, len(x), 1024):
        frames = min(1024, len(x)-start)
        if start <= HOLD*RATE < start+frames:
            synth.send_note_off(synth.s, HOLD*RATE-start, note, 64)
        x[start:start+frames] = synth.render(frames)
    assert np.isfinite(x).all() and abs(x).max() > 0
    return x


def compare(a, b, support):
    rows = []
    ceiling = max(q['total_db'] for q in a['stages'][:4])
    for i, (ref, model) in enumerate(zip(a['stages'], b['stages'])):
        valid = ref['stage'][1] <= support
        row = dict(stage=ref['stage'], recorded_support=valid,
            reference_above_floor=ref['total_db'] > ceiling-60,
            level_delta_db=model['total_db']-ref['total_db'],
            band_delta_db=(np.array(model['bands_db'])-ref['bands_db']).tolist(),
            subfundamental_delta_db=model['subfundamental_db']-ref['subfundamental_db'],
            stereo_side_mid_delta_db=model['stereo_side_mid_db']-ref['stereo_side_mid_db'])
        if ref['partials_db'] is not None:
            row['partial_delta_db']=(np.array(model['partials_db'])-ref['partials_db']).tolist()
            row['partial_reliable']=[v > ceiling-55 for v in ref['partials_db']]
        rows.append(row)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--sfz', type=Path, required=True)
    parser.add_argument('--dll', type=Path, required=True)
    parser.add_argument('--mapping', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=2)
    a = parser.parse_args()
    a.work.mkdir(parents=True, exist_ok=True)
    a.output.mkdir(parents=True, exist_ok=True)
    frozen = json.loads(a.model.read_text())
    assert digest(a.binary) == frozen['binary_sha256']
    bank = json.loads(a.mapping.read_text())
    sources = {(r['root'], r['layer']):r for r in bank['layers']}
    sampler = load(Path(__file__).with_name('render-bitklavier-steinway-midi.py'), 'original_sampler')
    synth = sampler.Engine(a.dll, a.sfz)
    assert synth.get_num_regions(synth.s) == bank['region_count']
    pairs = sorted({(n,v) for n in ROOTS for v in VELOCITIES} | MAPPED)
    reference_cache = {}
    models = {}
    gains = []
    with ThreadPoolExecutor(max_workers=a.jobs) as pool:
        futures = {pair:pool.submit(render_model, a.binary, frozen, a.work/f'{pair[0]}-{pair[1]}', *pair)
                   for pair in sorted(CALIBRATION)}
        for pair in sorted(CALIBRATION):
            native = read_model(futures[pair].result())
            reference = render_reference(synth, *pair)
            reference_cache[pair] = reference
            models[pair] = native
            x, _ = aligned(native)
            y, _ = aligned(reference)
            gains.append(float(db(power(y[round(.05*RATE):round(.3*RATE)]))
                               -db(power(x[round(.05*RATE):round(.3*RATE)]))))
        gain_db = float(np.median(gains))
        gain = 10**(gain_db/20)
        print('COMMON GAIN', gain_db, 'dB, calibrated on 14 frozen training pairs', flush=True)
        assert len(CALIBRATION) == 14
        futures.update({pair:pool.submit(render_model, a.binary, frozen, a.work/f'{pair[0]}-{pair[1]}', *pair)
                        for pair in pairs if pair not in futures})
        report = dict(policy=dict(reference='Original Stage ORTF samples reproduced by frozen SFZ; exact roots primary, 12 interpolated probes separately labelled.',
            gain='Single median 50-300ms calibration on 14 old training pairs; no matching per note/channel/window.',
            sustain='12s held, note-off velocity 64, no pedals, 4s tail; sampler note-off is not measured real-piano damping.',
            source_duration='File support bounds comparisons; recording note-off times are unknown. Late decay includes capture and possible original release.',
            masks='Only reliability masks use reference-relative floors: 60dB total / 55dB partial; excessive model energy is retained by spectral union mask.',
            phase='No samplewise waveform/phase subtraction between recordings; both-channel powers are averaged before logarithms.',
            spectra='512/2048/8192 sample windows. Short windows cannot resolve low-frequency modes independently; zero padding does not add resolution.',
            velocity='16 library layers; MIDI representatives 4,12,...124. Library/player dynamics are an audio reference, not measured hammer speeds.'),
            model_sha256=digest(a.model), binary_sha256=digest(a.binary), sfz_sha256=digest(a.sfz),
            mapping_sha256=digest(a.mapping), dll_sha256=digest(a.dll), tool_sha256=digest(Path(__file__)),
            gain_db=gain_db, calibration_pairs=sorted(CALIBRATION), bands_hz=BANDS, stages_s=STAGES,
            rows={}, audios={}, checks={})
        clips = {}
        for index, pair in enumerate(pairs):
            n, v = pair
            native = models.pop(pair, None)
            if native is None:
                native = read_model(futures[pair].result())
            model = native*gain
            reference = reference_cache.pop(pair, None)
            if reference is None:
                reference = render_reference(synth, n, v)
            root = min(ROOTS, key=lambda q:abs(n-q))
            layer = v//8+1
            source = sources[root, layer]
            path = Path(source['file'])
            assert digest(path) == bank['sources_sha256'][str(path)]
            raw, rate = sf.read(path, frames=RATE*SECONDS, always_2d=True)
            assert rate == RATE and raw.shape[1] == 2
            raw_onset = onset(raw)/RATE
            support = min(11.5, source['frames']/RATE-raw_onset)
            fa = features(reference, n)
            fb = features(model, n, fa['centers_hz'])
            delta = compare(fa, fb, support)
            row = dict(note=n, velocity=v, source_root=root, source_layer=layer,
                transposition_semitones=n-root, source_file=str(path), source_sha256=digest(path),
                source_seconds=source['frames']/RATE, source_gain_db=source['velocity_gain_db'][str(v)],
                calibration_pair=pair in CALIBRATION, reference=fa, model=fb, differences=delta,
                spectrum=multi_spectrum(reference, model, support),
                release_comparison='Controlled sampler envelope and recorded release noises; not a physical damping ground truth.')
            report['rows'][f'{n}:{v}'] = row
            if n in (60,65,67,72,79,84,91,96) and v in (60,84,124):
                clips[pair] = (reference[:3*RATE], model[:3*RATE])
            if (index+1)%16 == 0 or index+1 == len(pairs):
                print('MEASURED', index+1, '/', len(pairs), 'latest', n, v, flush=True)
                (a.work/'progress.json').write_text(json.dumps(dict(done=index+1,total=len(pairs),gain_db=gain_db)),encoding='utf-8')
        synth.close()
    # Checks expose the invariances that made previous objectives incomplete.
    test = reference[:RATE]
    unshifted = multi_spectrum(test, test, 1)
    shifted = multi_spectrum(test, test*10, 1)
    assert unshifted[0]['stage_mae_db'][2] < 1e-9
    assert shifted[0]['stage_mae_db'][2] > 19.9
    anti = np.column_stack([test[:,0], -test[:,0]])
    assert abs(power(anti)-power(test[:,0])) < 1e-10
    report['checks'] = dict(finite_native_and_reference=True, exact_main_pairs=480,
        mapped_extra_pairs=12, single_gain=True, native_release_events=True,
        plus20db_penalty=shifted[0]['stage_mae_db'][2], identical_error=unshifted[0]['stage_mae_db'][2],
        opposite_channel_phases_preserve_energy=True)
    # Paired previews use the one calibration and one safety trim across every file.
    peak = max(float(abs(x).max()) for pair in clips.values() for x in pair)
    trim = min(1, .85/peak)
    report['audition_common_trim_db'] = 20*math.log10(trim)
    for label, notes in [('medios',(60,65,67,72)), ('agudos',(72,79,84,91,96))]:
        for which, name in enumerate(('Steinway', 'Modelo-14B')):
            pieces = []
            for n in notes:
                for v in (60,84,124):
                    z = clips[n,v][which].copy()*trim
                    z[-662:] *= np.linspace(1,0,662)[:,None]
                    pieces.extend((z, np.zeros((round(.4*RATE),2))))
            wav = a.output/f'{name}-{label}-evaluacion.wav'
            sf.write(wav, np.concatenate(pieces), RATE, subtype='PCM_24')
            report['audios'][wav.name] = dict(notes=notes, velocities=[60,84,124],
                clip_seconds=3,gap_seconds=.4,tail_fade_ms=15,sha256=digest(wav),
                policy='3s held excerpts, not a note-off demonstration; mapped F4/G4/G5/G6 explicitly use nearest sample +/-1 semitone.')
    assert digest(a.binary) == frozen['binary_sha256']
    (a.output/'evaluation.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    with (a.output/'per-note.csv').open('w',newline='',encoding='utf-8') as stream:
        writer = csv.writer(stream)
        writer.writerow(['note','velocity','source_root','mapped','stage_start','stage_end','supported',
            'reference_above_floor','level_delta_db','subfundamental_delta_db','side_mid_delta_db',
            *[f'band_{lo}_{hi}_delta_db' for lo,hi in BANDS]])
        for row in report['rows'].values():
            for d in row['differences']:
                writer.writerow([row['note'],row['velocity'],row['source_root'],row['transposition_semitones'],
                    *d['stage'],d['recorded_support'],d['reference_above_floor'],d['level_delta_db'],
                    d['subfundamental_delta_db'],d['stereo_side_mid_delta_db'],*d['band_delta_db']])
    print('DONE', a.output, 'pairs',len(pairs),flush=True)


if __name__ == '__main__':
    main()
