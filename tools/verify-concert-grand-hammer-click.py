"""Verify unchanged contact/string/board traces when added strike noises are removed."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('before', 'after', 'binary', 'work', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    a = p.parse_args()
    models = {label: json.loads(path.read_text(encoding='utf-8')) for label, path in
              (('Antes', a.before), ('Sin-click', a.after))}
    tuning = {label: {**m['model']['tuning'], **m['overrides']} for label,m in models.items()}
    changed = {key for key in set(tuning['Antes']) | set(tuning['Sin-click'])
               if tuning['Antes'].get(key) != tuning['Sin-click'].get(key)}
    assert changed == {'KNOCK_LEVEL', 'THUMP_BASE'}
    assert models['Antes']['model']['params'] == models['Sin-click']['model']['params']
    assert all(sha(a.binary) == model['model']['binary_sha256'] for model in models.values())
    pairs = ((36,124), (60,60), (60,124), (72,124), (90,124), (96,124))

    def render(label):
        folder = a.work / label
        folder.mkdir(parents=True, exist_ok=True)
        (folder/'grid').mkdir(exist_ok=True)
        config = folder / 'tuning.txt'
        config.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning[label].items()))+'\n')
        env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_TUNING=str(config.resolve()), CG_GRID=','.join(f'{n}:{v}' for n,v in pairs),
                   CG_STAGE_DIR=str((folder/'stages').resolve()), CG_RENDER_DIR=str((folder/'grid').resolve()),
                   CG_PARAMS=','.join(f'{k}={v}' for k,v in models[label]['model']['params'].items()),
                   CG_PRESET='concert', CG_RATE='44100', CG_WARMUP_MS='200', CG_LEFT='1',
                   RUST_MIN_STACK='33554432')
        with (folder/'render.log').open('w') as log:
            subprocess.run([str(a.binary.resolve()), '--exact', 'tests::render_reference_wavs', '--ignored', '--nocapture'],
                           env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        assert 'complaints: []' in (folder/'render.log').read_text()
        print('Traced '+label, flush=True)
        return folder/'stages'

    with ThreadPoolExecutor(max_workers=2) as pool:
        directories = dict(zip(models, pool.map(render, models)))
    rows = {}
    fields = ('bridge_sum', 'board_drive_sum', 'board_raw', 'board_rim', 'board_direct', 'board_right_direct')
    for n,v in pairs:
        traces = {label: np.genfromtxt(folder/f'stages{n:03}v{v}.csv', delimiter=',', names=True)
                  for label,folder in directories.items()}
        before, after = traces['Antes'], traces['Sin-click']
        checks = {field: bool(np.array_equal(before[field], after[field])) for field in fields}
        assert all(checks.values()), (n, v, checks)
        assert np.count_nonzero(after['keybed_direct']) == 0
        assert np.count_nonzero(before['keybed_direct']) > 0
        for prefix in ('contact', 'modal', 'loading', 'strike'):
            paths = [folder/f'{prefix}{n:03}v{v}.csv' for folder in directories.values()]
            checks[prefix] = paths[0].read_bytes() == paths[1].read_bytes()
            assert checks[prefix], (n, v, prefix)
        rows[f'{n}:{v}'] = checks
    report = dict(policy=__doc__, before_sha256=sha(a.before), after_sha256=sha(a.after),
                  binary_sha256=sha(a.binary), changed_tuning=sorted(changed), trace_seconds=.3,
                  fresh_strike_mechanical_trace_zero=True, exact_contact_and_board_parity=rows,
                  limitation='Trace parity concerns fresh attacks; full MIDI renders separately preserve pedal/release events.')
    a.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print('PASS: physical contact, modal states, bridge loading and board audio unchanged.', flush=True)


if __name__ == '__main__':
    main()
