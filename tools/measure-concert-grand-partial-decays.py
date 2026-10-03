"""Measure local partial-envelope slopes, without assuming one decay stage.

Stereo energy avoids mono cancellation. Centered Hann demodulation resolves
at least eight fundamental periods; these are offline measurements, not DSP.
Local slope and its residual describe acoustic envelopes, not uniquely the
intrinsic string loss. Reference-only support flags retain missing model terms.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
from scipy.signal import fftconvolve
import soundfile as sf

RATE = 44100
WINDOWS = ((.15, .4), (.45, .8), (.9, 1.4))


def envelope(x, frequency, fundamental, rate=RATE):
    x = np.asarray(x, dtype=float)
    cycles = max(8, int(np.ceil(.04*fundamental)))
    width = int(round(cycles*rate/fundamental)) | 1
    window = np.hanning(width)
    carrier = np.exp(-2j*np.pi*frequency*np.arange(len(x))/rate)
    amplitudes = fftconvolve(x*carrier[:, None], window[:, None], mode='same', axes=0)*2/window.sum()
    indices = np.arange(round(.1*rate), round(1.451*rate), round(.01*rate))
    assert indices[0] > width//2 and indices[-1]+width//2 < len(x)
    power = np.mean(abs(amplitudes[indices])**2, axis=1)
    return indices/rate, 10*np.log10(np.maximum(power, 1e-30)), width/rate


def slopes(times, levels):
    rows = []
    for lo, hi in WINDOWS:
        mask = (times >= lo)&(times < hi)
        t, y = times[mask], levels[mask]
        fit = np.polyfit(t-t.mean(), y, 1)
        residual = y-np.polyval(fit, t-t.mean())
        rows.append(dict(window_s=[lo, hi], slope_db_per_s=float(fit[0]),
                         level_db=float(y.mean()), residual_rms_db=float(np.sqrt(np.mean(residual**2)))))
    return rows


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('decay_body', Path(__file__).with_name('identify-concert-grand-body.py'))
    b = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(b)
    provenance = json.loads((a.input/'reference-manifest.json').read_text())
    report = dict(policy=__doc__, windows_s=WINDOWS, rows={})
    for n, v in b.TRAIN+b.PITCH_TEST+b.VELOCITY_TEST:
        directory = a.input/'capture'/f'{n}-{v}'
        capture = json.loads((directory/'manifest.json').read_text())
        assert capture['binary_sha256'] == b.SHA
        actual = np.array(b.FIT.AUDIT.load(directory/'performance.f32'), dtype=float)
        raw = directory/'bridge-drive.f32'
        force = np.memmap(raw, dtype='<f4', mode='r').reshape(-1, 16).sum(axis=1, dtype=float)
        path = a.input/'reference'/f'{n}-{v}.wav'
        meta = json.loads(path.with_suffix('.json').read_text())
        assert meta['source'] == provenance['sources'][f'{n}:{v}']
        assert all(meta[k] == provenance[k] for k in ('mapping_sha256', 'sfz_sha256', 'dll_sha256'))
        assert b.FIT.AUDIT.sha(Path(meta['source']['file'])) == meta['source']['sha256']
        target, rate = sf.read(path, always_2d=True)
        assert rate == RATE
        traces = dict(Actual=b.FIT.EVAL.aligned(actual)[0], Steinway=b.FIT.EVAL.aligned(target)[0],
                      Fuerza=b.FIT.EVAL.aligned(np.repeat(force[:, None], 2, axis=1))[0])
        f0 = 440*2**((n-69)/12)
        result = dict(partials=[], binary_sha256=b.SHA, audio_sha256=b.FIT.AUDIT.sha(directory/'performance.f32'),
                      force_sha256=b.FIT.AUDIT.sha(raw), reference_sha256=b.FIT.AUDIT.sha(path))
        for h in (1, 2, 3):
            row = dict(partial=h, observations={})
            for label, x in traces.items():
                hz, power = b.FIT.EVAL.psd(x[round(.05*RATE):round(.5*RATE)])
                bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
                frequency = hz[bins[np.argmax(power[bins])]]
                times, levels, width = envelope(x, frequency, f0)
                row['observations'][label] = dict(frequency_hz=float(frequency), smoothing_s=width,
                    times_s=times.tolist(), levels_db=levels.tolist(), slopes=slopes(times, levels))
            result['partials'].append(row)
        reference_h1 = result['partials'][0]['observations']['Steinway']['slopes'][0]['level_db']
        for row in result['partials']:
            for index, window in enumerate(row['observations']['Steinway']['slopes']):
                window['supported_relative_to_early_H1'] = window['level_db'] > reference_h1-40
                for label in ('Actual', 'Fuerza'):
                    row['observations'][label]['slopes'][index]['reference_supported'] = window['supported_relative_to_early_H1']
        report['rows'][f'{n}:{v}'] = result
    (a.output/'partial-decays.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    for key in ('60:60', '60:110', '84:124'):
        print(key, json.dumps({label: data['slopes'] for label, data in report['rows'][key]['partials'][0]['observations'].items()}), flush=True)


if __name__ == '__main__':
    main()
