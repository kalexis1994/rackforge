"""Paired attack ablations on isolated velocities and the exact Campanella passage."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('candidate', 'binary', 'events', 'work', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--profiles', type=Path)
    parser.add_argument('--notes', default='60,65,67,72,90')
    parser.add_argument('--velocities', default='40,60,84,124')
    parser.add_argument('--hold', type=int, default=2)
    parser.add_argument('--seconds', type=int, default=3)
    parser.add_argument('--audition-seconds', type=float, default=1.5)
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    e = module(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'), 'evaluation')
    e.HOLD = args.hold
    e.SECONDS = args.seconds
    assert 0 < args.audition_seconds <= args.hold < args.seconds
    chosen = json.loads(args.candidate.read_text(encoding='utf-8'))
    model = {**chosen['model'], 'tuning': {**chosen['model']['tuning'], **chosen['overrides']}}
    assert e.digest(args.binary) == model['binary_sha256']
    profiles = json.loads(args.profiles.read_text()) if args.profiles else {
        'Actual': {}, 'Sin-mecanismo': {'MECHANICAL_EMISSION_GAIN': 0},
        'Sin-contacto-tabla': {'CONTACT_BOARD_GAIN': 0}, 'Sin-impacto-extra': {'IMPACT_CLANG': 0},
    }
    notes = tuple(int(note) for note in args.notes.split(','))
    velocities = tuple(int(v) for v in args.velocities.split(','))
    pairs = [(note, velocity) for note in notes for velocity in velocities]
    passages = ((36, 44), (92, 100))
    total = 101 * e.RATE
    event_text = '\n'.join(line for line in args.events.read_text().splitlines() if int(line.split()[0]) < total) + '\n'

    def render_profile(item):
        name, overrides = item
        folder = args.work / name
        folder.mkdir(exist_ok=True)
        changes = overrides.get('tuning', {}) if 'params' in overrides else overrides
        frozen = {**model, 'tuning': {**model['tuning'], **changes},
                  'params': {**model['params'], **overrides.get('params', {})}}
        raw_notes = {pair: e.render_model(args.binary, frozen, folder / f'{pair[0]}-{pair[1]}', *pair) for pair in pairs}
        raw = folder / 'performance.f32'
        events, tuning = folder / 'events.txt', folder / 'tuning.txt'
        events.write_text(event_text)
        tuning.write_text('\n'.join(f'{key} = {value}' for key, value in sorted(frozen['tuning'].items())) + '\n')
        signature = dict(binary=e.digest(args.binary), tuning=frozen['tuning'], params=frozen['params'],
                         events=e.digest(events), frames=total)
        marker = folder / 'passage.json'
        if not (marker.exists() and json.loads(marker.read_text()) == signature and raw.exists() and raw.stat().st_size == total * 8):
            env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CG_')}
            env.update(CG_EVENT_FILE=str(events.resolve()), CG_TOTAL_FRAMES=str(total), CG_RATE=str(e.RATE),
                       CG_RENDER_DIR=str(folder.resolve()), CG_TUNING=str(tuning.resolve()), CG_PRESET='concert',
                       CG_PARAMS=','.join(f'{key}={value}' for key, value in frozen['params'].items()), RUST_MIN_STACK='33554432')
            with (folder / 'render.log').open('w') as log:
                subprocess.run([str(args.binary.resolve()), '--exact', 'tests::render_reference_wavs', '--ignored', '--nocapture'],
                               env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            assert 'complaints: []' in (folder / 'render.log').read_text()
            marker.write_text(json.dumps(signature, indent=2))
        assert raw.stat().st_size == total * 8
        print('Finished ' + name, flush=True)
        return name, dict(notes=raw_notes, passage=raw)

    with ThreadPoolExecutor(max_workers=2) as pool:
        results = dict(pool.map(render_profile, profiles.items()))
    sampler = module(Path(__file__).with_name('render-bitklavier-steinway-midi.py'), 'sampler')
    synth = sampler.Engine(Path('local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'),
                           Path('local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz'))
    references = {pair: e.render_reference(synth, *pair) for pair in pairs}
    synth.close()
    # Frozen sampler-comparison gain. No per-note normalization.
    gain_db = 26.401176937389373
    gain = 10 ** (gain_db / 20)
    signals = {name: {pair: e.read_model(path) * gain for pair, path in row['notes'].items()} for name, row in results.items()}
    signals['Steinway-sampler'] = references
    maximum = max(float(np.abs(x).max()) for notes in signals.values() for x in notes.values())
    trim = min(1, .85 / maximum)
    high = butter(4, (1800, 8000), btype='bandpass', fs=e.RATE, output='sos')
    low = butter(4, (200, 1200), btype='bandpass', fs=e.RATE, output='sos')

    def features(x):
        aligned, onset = e.aligned(x)
        hf = sosfilt(high, aligned, axis=0)
        body = sosfilt(low, aligned, axis=0)
        attack = hf[:round(.025 * e.RATE)]
        bloom = hf[round(.025 * e.RATE):round(.15 * e.RATE)]
        return dict(onset_frame=onset,
                    hf_initial_db=float(e.db(e.power(attack))),
                    hf_bloom_db=float(e.db(e.power(bloom))),
                    hf_initial_over_bloom_db=float(e.db(e.power(attack) / max(e.power(bloom), 1e-18))),
                    body_50_200ms_db=float(e.db(e.power(body[round(.05*e.RATE):round(.2*e.RATE)]))),
                    peak=float(np.abs(x).max()))

    report = dict(candidate_sha256=e.digest(args.candidate), binary_sha256=e.digest(args.binary),
                  base_tuning=model['tuning'], params=model['params'], overrides=profiles, pairs=pairs,
                  gain_db=gain_db, common_trim_db=float(20*np.log10(trim)), rows={},
                  policy='Independent fresh notes, identical gain; nearest-root sampler mapping for F4/G4. '
                         'Passages retain all events from t=0; exported t=36..44s and 92..100s. Ablations are diagnostics.')
    for name, notes in signals.items():
        report['rows'][name] = {f'{n}:{v}': features(notes[n, v]) for n, v in pairs}
        chunks = []
        for pair in pairs:
            clip = notes[pair][:round(args.audition_seconds*e.RATE)].copy() * trim
            clip[-441:] *= np.linspace(1, 0, 441)[:, None]
            chunks.extend((clip, np.zeros((round(.25*e.RATE), 2))))
        sf.write(args.output / (name + '-intensidades.wav'), np.concatenate(chunks), e.RATE, subtype='PCM_24')
    passage_gain_db = 25.300256  # same full-performance gain, common to all ablations
    passage_gain = 10 ** (passage_gain_db / 20)
    for name, row in results.items():
        raw = e.read_model(row['passage'])
        for lo, hi in passages:
            x = raw[lo*e.RATE:hi*e.RATE] * passage_gain
            assert np.isfinite(x).all() and np.abs(x).max() < 1
            sf.write(args.output / (name + f'-Campanella-{lo}s-{hi}s.wav'), x, e.RATE, subtype='PCM_24')
    reference_piece = Path('C:/SHARED/SteinwayD-MIDI/SteinwayD-StageORTF-liszt-la-campanella.wav')
    with sf.SoundFile(reference_piece) as stream:
        assert stream.samplerate == e.RATE
        for lo, hi in passages:
            stream.seek(lo*e.RATE)
            sf.write(args.output / f'Steinway-sampler-Campanella-{lo}s-{hi}s.wav', stream.read((hi-lo)*e.RATE), e.RATE, subtype='PCM_24')
    report['passages_seconds'] = passages
    baseline = signals['Actual']
    report['difference_energy'] = {}
    for name, notes in signals.items():
        if name in ('Actual', 'Steinway-sampler'):
            continue
        report['difference_energy'][name] = {f'{n}:{v}': dict(
            initial_relative_db=float(e.db(e.power((baseline[n,v]-notes[n,v])[:round(.025*e.RATE)]) /
                                         max(e.power(baseline[n,v][:round(.025*e.RATE)]), 1e-18))),
            body_change_db=report['rows'][name][f'{n}:{v}']['body_50_200ms_db']-
                           report['rows']['Actual'][f'{n}:{v}']['body_50_200ms_db']) for n,v in pairs}
    (args.output / 'diagnostico.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    (args.output / 'LEEME.txt').write_text(
        f'Pruebas de ataque: notas MIDI {notes}; cada nota a velocidades {velocities}.\n'
        f'{args.audition_seconds} segundos por nota y 0,25 segundos de silencio. Mismo volumen para todas las variantes.\n'
        'Los fragmentos de La Campanella corresponden a 36–44 s y 92–100 s del audio completo.\n'
        'Sin-mecanismo, Sin-contacto-tabla y Sin-impacto-extra son pruebas de diagnóstico.\n', encoding='utf-8')
    print('DONE attack audit', flush=True)


if __name__ == '__main__':
    main()
