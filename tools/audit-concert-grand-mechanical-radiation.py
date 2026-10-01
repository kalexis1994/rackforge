"""Audit acoustic mechanism routes without normalizing individual notes.

The counterfactual with synthetic attack noise disabled must leave the held
string/contact signal identical. Differences include subsequent acoustic tails.
This diagnoses model sources; it does not identify mechanism sound in samples.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--research-tools', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('refit', args.research_tools/'concert-grand-refit-v2.py')
    refit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(refit)
    model = json.loads(args.model.read_text())
    sha = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    if model.get('binary_sha256') != sha:
        raise ValueError('Model and renderer provenance do not match')
    pairs = [(note, velocity) for note in (48, 60, 72, 84, 96) for velocity in (60, 124)]
    profiles = {
        'legacy': {'MECHANICAL_RADIATION': 0},
        'radiation': {'MECHANICAL_RADIATION': 1},
        'legacy-no-action': {'MECHANICAL_RADIATION': 0, 'KNOCK_LEVEL': 0, 'THUMP_BASE': 0},
        'radiation-no-action': {'MECHANICAL_RADIATION': 1, 'KNOCK_LEVEL': 0, 'THUMP_BASE': 0},
    }

    def render(item):
        name, overrides = item
        refit.render_candidate(args.binary, args.output/name, pairs, model['params'],
            tuning={**model['tuning'], **overrides}, warmup_ms=200)
        print('rendered', name, flush=True)

    with ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(render, profiles.items()))
    energy = lambda x: max(float(np.mean(x*x)), 1e-24)
    rows = []
    for note, velocity in pairs:
        signals = {}
        for profile in profiles:
            signal, rate = sf.read(args.output/profile/f'model{note:03}v{velocity}.wav')
            assert rate == 44100 and np.isfinite(signal).all()
            signals[profile] = signal
        x, y = signals['legacy'], signals['radiation']
        a, b = signals['legacy-no-action'], signals['radiation-no-action']
        assert np.array_equal(a, b), 'Held string/contact audio changed with action noise disabled'
        old_mechanism, new_mechanism = (x-a)[:13230], (y-b)[:13230]
        rows.append(dict(note=note, velocity=velocity, held_tone_counterfactual_identical=True,
            mechanical_reduction_db=10*np.log10(energy(new_mechanism)/energy(old_mechanism)),
            total_attack_change_db=10*np.log10(energy(y[:13230])/energy(x[:13230])),
            sustain_change_db=10*np.log10(energy(y[66150:154350])/energy(x[66150:154350]))))

    pedal_rows = []
    for profile in ('legacy', 'radiation'):
        for distance in (0.66, 2.64, 8.0):
            directory = args.output/f'pedal-{profile}-{distance}'
            directory.mkdir(exist_ok=True)
            events = directory/'events.txt'
            events.write_text('0 176 64 127\n88200 176 64 0\n')
            tuning = {**model['tuning'], **profiles[profile]}
            settings = directory/'tuning.txt'
            settings.write_text('\n'.join(f'{k} = {v}' for k, v in sorted(tuning.items()))+'\n')
            env = refit.clean_render_environment()
            env.update(CG_EVENT_FILE=str(events.resolve()), CG_TOTAL_FRAMES='220500',
                CG_RENDER_DIR=str(directory.resolve()), CG_TUNING=str(settings.resolve()),
                CG_PRESET='concert', CG_PARAMS=f'25={distance}', CG_RATE='44100', RUST_MIN_STACK='33554432')
            with (directory/'render.log').open('w') as log:
                subprocess.run([str(args.binary.resolve()), '--exact', 'tests::render_reference_wavs',
                    '--ignored', '--quiet'], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            audio = np.fromfile(directory/'performance.f32', dtype='<f4').reshape(-1, 2)
            assert len(audio) == 220500 and np.isfinite(audio).all()
            pedal_rows.append(dict(profile=profile, microphone_m=distance,
                attack_rms_db=10*np.log10(energy(audio[:13230])), peak=float(np.abs(audio).max())))
    result = dict(binary_sha256=sha, model=model, attack_rows=rows, pedal_rows=pedal_rows,
        policy='10 held probes; 300 ms attack and 1.5-3.5 s sustain; paired source ablations; pedal alone with identical CC64. Exploratory acoustic calibration, not a fitted radiation efficiency.')
    (args.output/'results.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result['attack_rows']), flush=True)
    print(json.dumps(pedal_rows), flush=True)


if __name__ == '__main__':
    main()
