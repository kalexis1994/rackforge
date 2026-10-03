"""Curate the fixed strong-note unison pilot without changing export gains."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

import numpy as np
import soundfile as sf


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    report = json.loads((a.input/'report.json').read_text())
    assert report['export_gain_db'] == 0 and report['factory_changed'] is False
    variant = 'I-unisono-mas-visible'
    exports = dict(policy=__doc__, pilot_report_sha256=sha(a.input/'report.json'),
                   gain_db=0, velocity=124, note_hold_s=2,
                   listener_verdict='B rejected: flanger-like sound; archived diagnostic only',
                   order=[dict(label='A: control anterior', start_s=0),
                          dict(label='B: unisono experimental', start_s=4.4),
                          dict(label='C: Steinway', start_s=8.8)], files={})
    for n, name in ((60, 'C4'), (72, 'C5'), (84, 'C6'), (90, 'Fsharp6')):
        for family in ('Carga-fisica', 'Control-sin-carga'):
            assert report['profiles'][f'{family}/{variant}']['tuning']['UNISON_WIDTH'] == .65
            paths = [a.input/family/f'{n}-124-Base.wav', a.input/family/f'{n}-124-{variant}.wav',
                     a.input/f'{n}-124-Steinway.wav']
            parts = []
            for path in paths:
                x, rate = sf.read(path, always_2d=True)
                assert rate == 44100 and x.shape == (176400, 2) and abs(x).max() < 1
                # Only a 10ms fade at the final release boundary; the held note
                # and attack are untouched, with identical treatment for A/B/C.
                x[-441:] *= np.linspace(1, 0, 441)[:, None]
                parts.extend((x, np.zeros((17640, 2))))
            path = a.output/f'{name}-{family}-A-B-Steinway.wav'
            sf.write(path, np.concatenate(parts), 44100, subtype='PCM_24')
            exports['files'][path.name] = dict(sha256=sha(path), inputs={str(p): sha(p) for p in paths},
                diagnostic_decoupling=family=='Control-sin-carga')
        for label, path in {
            'Unisono-control': a.input/'Control-sin-carga'/f'{n}-124-{variant}.wav',
            'Steinway': a.input/f'{n}-124-Steinway.wav',
            'Unisono-control-sostenida-12s': a.input/'Control-sin-carga'/f'{n}-124-{variant}-sostenida-12s.wav',
            'Steinway-sostenida-12s': a.input/f'{n}-124-Steinway-sostenida-12s.wav',
        }.items():
            target = a.output/f'{name}-{label}.wav'
            shutil.copyfile(path, target)
            assert sha(target) == sha(path)
            exports['files'][target.name] = dict(sha256=sha(target), input=str(path))
    (a.output/'comparacion.json').write_text(json.dumps(exports, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
