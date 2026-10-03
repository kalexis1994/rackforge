"""Factorial diagnosis of the listener-rejected unison pilot, not a new fit.

Keep stereo and unity export gain. Separate fixed pitch spread, secondary
string losses and reactive bridge loading. Harmonic colour motion below is
a descriptive baseline-relative measurement, not an audible-flanger score.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    tools = Path(__file__).resolve().parent
    audit = module(tools/'audit-concert-grand-duplex-fix.py', 'flanger_audit')
    subtle = module(tools/'audit-concert-grand-subtle-unison.py', 'flanger_subtle')
    fit = module(tools/'fit-concert-grand-current-physics.py', 'flanger_fit')
    digest = audit.sha(args.binary)
    assert digest == 'a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831'
    rate = 44100
    profiles = {
        'A-base': {'UNISON_WIDTH': 0, 'UNISON_EXTRA_LOSS': 2},
        'Solo-afinacion': {'UNISON_WIDTH': .65, 'UNISON_EXTRA_LOSS': 2},
        'Solo-perdidas': {'UNISON_WIDTH': 0, 'UNISON_EXTRA_LOSS': 1},
        'B-rechazada': {'UNISON_WIDTH': .65, 'UNISON_EXTRA_LOSS': 1},
        'B-sin-reactividad': {'UNISON_WIDTH': .65, 'UNISON_EXTRA_LOSS': 1,
                              'REACTIVE_BRIDGE': 0},
    }
    args.output.mkdir(parents=True, exist_ok=True)
    report = dict(policy=__doc__, renderer_sha256=digest, factory_changed=False,
                  listener_verdict={'B-rechazada': 'Rejected: flanger-like sound'},
                  export_gain_db=0, velocity=124, hold_s=2, profiles=profiles, notes={})
    for note, name in ((60, 'C4'), (84, 'C6')):
        arrays, identities = {}, {}
        for label, tuning in profiles.items():
            # Reuse the exact existing controls without copying their raw cache.
            cached = {'A-base': 'Base', 'B-rechazada': 'I-unisono-mas-visible'}.get(label)
            folder = (tools.parent/'local/concert-grand-subtle-unison/renders/Control-sin-carga'/cached/f'{note}-124'
                      if cached else args.work/label/f'{note}-124')
            settings = {'DIAGNOSTIC_STRING_DRAIN': 0, **tuning}
            if cached:
                # Existing manifests omit explicit factory defaults.
                settings = {'DIAGNOSTIC_STRING_DRAIN': 0} if label == 'A-base' else settings
            raw = audit.render(args.binary, f'0 144 {note} 124\n88200 128 {note} 64\n',
                               4*rate, folder, settings)
            x = np.array(audit.load(raw), dtype=float)
            assert np.isfinite(x).all() and abs(x).max() < 1
            arrays[label] = x
            identities[label] = dict(raw_sha256=audit.sha(raw), manifest=json.loads((folder/'manifest.json').read_text()))
            sf.write(args.output/f'{name}-{label}.wav', x, rate, subtype='PCM_24')
            print(f'{name}/{label}', flush=True)
        ref_path = tools.parent/'local/concert-grand-identified-body/reference'/f'{note}-124.wav'
        reference, ref_rate = sf.read(ref_path, always_2d=True)
        assert ref_rate == rate
        arrays['Steinway'] = reference
        identities['Steinway'] = dict(wav_sha256=audit.sha(ref_path))
        f0 = 440*2**((note-69)/12)
        ref_hz, ref_psd = fit.EVAL.psd(reference[round(.05*rate):round(.5*rate)])
        hz, psd = fit.EVAL.psd(arrays['A-base'][round(.05*rate):round(.5*rate)])
        rows = []
        for h in range(1, min(16, int(10000/f0))+1):
            bins = np.flatnonzero(abs(hz-h*f0) < .18*f0)
            frequency = float(hz[bins[np.argmax(psd[bins])]])
            ref_bins = np.flatnonzero(abs(ref_hz-h*f0) < .18*f0)
            ref_frequency = float(ref_hz[ref_bins[np.argmax(ref_psd[ref_bins])]])
            curves = {}
            for label, x in arrays.items():
                t, levels = subtle.partial_envelope(x, ref_frequency if label == 'Steinway' else frequency, f0, hold=2)
                curves[label] = levels
            early = (t >= .1)&(t < .5)
            rows.append(dict(harmonic=h, native_frequency_hz=frequency,
                             reference_frequency_hz=ref_frequency,
                             reference_peak_db=float(curves['Steinway'][early].max()),
                             curves=curves))
        strongest = max(row['reference_peak_db'] for row in rows)
        supported = [row for row in rows if row['reference_peak_db'] > max(strongest-35, -85)]
        interval = (t >= .25)&(t <= 1.8)
        metrics = {}
        for label in profiles:
            # One shared frequency and support mask for all native profiles.
            delta = np.stack([r['curves'][label]-r['curves']['A-base'] for r in supported])[:, interval]
            # Remove simultaneous overall level change, then each partial's
            # fixed offset. Only changes in relative spectral colour remain.
            colour = delta-np.median(delta, axis=0, keepdims=True)
            colour -= np.mean(colour, axis=1, keepdims=True)
            ranges = np.percentile(colour, 95, axis=1)-np.percentile(colour, 5, axis=1)
            level = lambda x: float(10*np.log10(np.mean(x[round(.5*rate):round(1.5*rate)]**2)+1e-30))
            metrics[label] = dict(relative_colour_rms_db=float(np.sqrt(np.mean(colour**2))),
                                 median_partial_colour_range_db=float(np.median(ranges)),
                                 max_partial_colour_range_db=float(ranges.max()),
                                 stereo_level_delta_db=level(arrays[label])-level(arrays['A-base']))
        supported_harmonics = {r['harmonic'] for r in supported}
        for row in rows:
            row['reference_supported'] = row['harmonic'] in supported_harmonics
            row['curves'] = {k:v.tolist() for k,v in row['curves'].items()}
        pieces, timeline = [], []
        for i, (label, x) in enumerate(arrays.items()):
            clip = x.copy()
            clip[-441:] *= np.linspace(1, 0, 441)[:, None]
            pieces.extend((clip, np.zeros((round(.4*rate), 2))))
            timeline.append(dict(label=label, start_s=4.4*i))
        sf.write(args.output/f'{name}-Diagnostico.wav', np.concatenate(pieces), rate, subtype='PCM_24')
        report['notes'][name] = dict(sources=identities, metrics=metrics, times_s=t.tolist(),
                                   partials=rows, timeline=timeline)
        print(json.dumps(dict(note=name, metrics=metrics)), flush=True)
    (args.output/'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    (args.output/'LEEME.txt').write_text(
        'B fue rechazada por el usuario: efecto parecido a flanger. No es candidata.\n'
        'Diagnostico: A a 0 s; solo afinacion a 4,4; solo perdidas a 8,8; B rechazada a 13,2; '
        'B sin reactividad a 17,6; Steinway a 22.\n'
        'Son controles sin drenaje, no una calibracion para instalar. Misma ganancia, sin EQ ni normalizacion.\n'
        'La medida de color espectral describe cambios entre parciales; no certifica ausencia de flanger audible.\n', encoding='utf-8')


if __name__ == '__main__':
    main()
