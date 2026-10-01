"""Validate the finite contact-loss experiment on fresh notes and a pedalled trill.

Onset contrasts are band-energy diagnostics, not a perceptual fidelity score.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import shutil
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt, sosfiltfilt


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def flat(path):
    data = json.loads(path.read_text(encoding='utf-8'))
    return {**data['model'], 'tuning': {**data['model']['tuning'], **data['overrides']}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('before-binary', 'after-binary', 'before-model', 'after-model',
                 'baseline', 'audition', 'reference', 'events', 'work'):
        parser.add_argument('--'+name, type=Path, required=True)
    a = parser.parse_args()
    a.work.mkdir(parents=True, exist_ok=True)
    before, after = flat(a.before_model), flat(a.after_model)
    assert sha(a.before_binary) == before['binary_sha256']
    assert sha(a.after_binary) == after['binary_sha256']
    assert before['tuning'] == after['tuning'] and before['params'] == after['params']
    rate = 44100
    frames = 207*rate
    baseline = np.memmap(a.baseline, dtype='<f4', mode='r').reshape(-1, 2)
    control = np.memmap(a.audition/'Control/performance.f32', dtype='<f4', mode='r').reshape(-1, 2)
    assert np.array_equal(control[:frames], baseline[:frames]), 'Disabled control changed accepted MIDI'

    spec = importlib.util.spec_from_file_location('evaluation', Path(__file__).with_name('evaluate-concert-grand-full-sound.py'))
    evaluation = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(evaluation)
    evaluation.HOLD, evaluation.SECONDS = 3, 4
    def fresh(pair):
        note, velocity = pair
        old = evaluation.render_model(a.before_binary, before, a.work/f'old-{note}-{velocity}', note, velocity)
        new_model = {**after, 'tuning': {**after['tuning'], 'RESTRIKE_OLD_RETENTION': 0.5}}
        new = evaluation.render_model(a.after_binary, new_model, a.work/f'new-{note}-{velocity}', note, velocity)
        assert old.read_bytes() == new.read_bytes(), f'Fresh note changed: {pair}'
        return dict(note=note, velocity=velocity, samples_equal=True, sha256=sha(new))
    with ThreadPoolExecutor(max_workers=2) as pool:
        fresh_checks = list(pool.map(fresh, [(36,124), (42,84), (60,60), (72,124), (90,124), (96,124)]))

    attacks = []
    for line in a.events.read_text().splitlines():
        fields = list(map(int, line.split()))
        if len(fields) == 4 and fields[1] & 0xf0 == 0x90 and fields[3] and 201*rate <= fields[0] < 203*rate:
            attacks.append(dict(frame=fields[0], seconds=fields[0]/rate, note=fields[2], velocity=fields[3]))
    reference, sample_rate = sf.read(a.reference, start=199*rate, stop=206*rate, dtype='float64')
    assert sample_rate == rate
    paths = {'Actual': a.baseline, **{label: a.audition/label/'performance.f32' for label in ('Contacto-70', 'Contacto-50')}}
    signals = {label: np.asarray(np.memmap(path, dtype='<f4', mode='r').reshape(-1,2)[199*rate:206*rate], dtype='float64')
               for label, path in paths.items()}
    signals['Steinway'] = reference
    result = {}
    gain = 10**(25.120884443707872/20)
    for label, signal in signals.items():
        assert np.isfinite(signal).all()
        metrics = {}
        for band in ((100,600), (600,1800), (1800,8000)):
            y = sosfilt(butter(4, band, fs=rate, btype='bandpass', output='sos'), signal, axis=0)
            rows = []
            for attack in attacks:
                frame = attack['frame']-199*rate
                pre = float(np.mean(y[frame-round(.040*rate):frame-round(.005*rate)]**2))
                post = float(np.mean(y[frame+round(.005*rate):frame+round(.045*rate)]**2))
                rows.append({**attack, 'contrast_db': float(10*np.log10(max(post,1e-30)/max(pre,1e-30))),
                             'pre_power_db': float(10*np.log10(max(pre,1e-30))) +
                                 (0 if label == 'Steinway' else 25.120884443707872)})
            metrics[str(band)] = dict(median_contrast_db=float(np.median([row['contrast_db'] for row in rows])),
                                     median_pre_power_db=float(np.median([row['pre_power_db'] for row in rows])), rows=rows)
        result[label] = metrics
        export = signal if label == 'Steinway' else signal*gain
        assert abs(export).max() < 1
        sf.write(a.audition/f'Nocturno-199s-206s-{label}.wav', export, rate, subtype='PCM_24')
    comparison = np.concatenate([signals['Actual']*gain, np.zeros((rate,2)), signals['Contacto-70']*gain])
    sf.write(a.audition/'Nocturno-Antes-Despues.wav', comparison, rate, subtype='PCM_24')
    for label,retention in [('Contacto-70',0.7), ('Contacto-50',0.5)]:
        candidate = json.loads(a.after_model.read_text(encoding='utf-8'))
        candidate['name'] = label + ' — perdida efectiva de la vibracion anterior'
        candidate['model']['label'] = candidate['name']
        candidate['overrides']['RESTRIKE_OLD_RETENTION'] = retention
        candidate['selected_for_production'] = False
        (a.audition/(label+'-modelo.json')).write_text(json.dumps(candidate,indent=2),encoding='utf-8')
    source = Path('local/concert-grand-coherent-unison/src/lib.rs')
    if source.exists():
        shutil.copyfile(source, a.audition/'fuente-investigacion.rs')
    report = dict(default_full_history_samples_equal=True, comparison_frames=frames,
                  fresh_notes=fresh_checks, before_binary_sha256=sha(a.before_binary),
                  after_binary_sha256=sha(a.after_binary), events_sha256=sha(a.events),
                  attack_count=len(attacks), metrics=result,
                  limitation='Effective contact retention, not a measured collision model. Same MIDI/pedal/gain; band contrasts are diagnostics only.')
    camp = a.audition/'Campanella-control'
    if (camp/'Contacto-70/performance.f32').exists():
        target = 4295673
        sos = butter(4, (8000,18000), fs=rate, btype='bandpass', output='sos')
        powers = {}
        for label,path in [('Actual',a.baseline.parent.parent/'liszt-la-campanella/performance.f32'),
                           ('Contacto-70',camp/'Contacto-70/performance.f32')]:
            raw = np.memmap(path, dtype='<f4', mode='r').reshape(-1,2)
            filtered = sosfiltfilt(sos,np.asarray(raw[target-2205:target+2205],dtype='float64'),axis=0)
            powers[label] = float(np.mean(filtered[2205:2249]**2))
        change = float(10*np.log10(powers['Contacto-70']/powers['Actual']))
        report['campanella_97_40755_hf_first_ms_change_db'] = change
        report['campanella_hf_first_ms_power'] = powers
        report['campanella_click_gate_pass'] = change < 3
    (a.audition/'validacion.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    (a.audition/'LEEME.txt').write_text(
        'Prueba experimental, no seleccionada para el plugin.\n'
        'Nocturno-Antes-Despues.wav: 0-7 s modelo aceptado; 7-8 s silencio; 8-15 s Contacto-70.\n'
        'El recorte corresponde a 3:19-3:26. El trino señalado está a 2-4 s de cada recorte.\n'
        'Cada variante se renderizó desde el principio, con todas las notas y pedal del MIDI.\n'
        'Ganancia común: 25.120884443707872 dB. Sin EQ, compresor ni normalización individual.\n'
        'Contacto-70 conserva el 70% de la vibración previa al reiterar; el nuevo golpe queda completo.\n'
        'El dúplex y los ruidos mecánicos no reciben esa pérdida. No se cambian las colas de otras teclas.\n'
        'Contacto-50 es una ablación más fuerte; menor fondo no garantiza mejor articulación.\n'
        'La prueba no resuelve completamente la interacción del martillo con la cuerda ya en movimiento.\n'
        'Steinway: bitKlavier Grand, Stage Lid ORTF. CC BY 4.0, créditos adjuntos.\n'
        'validacion.json contiene las métricas y el resultado de control del ataque de Campanella.\n', encoding='utf-8')
    print(json.dumps({label: {band: {k:v for k,v in metrics.items() if k!='rows'} for band,metrics in bands.items()}
                      for label,bands in result.items()}, indent=2), flush=True)


if __name__ == '__main__':
    main()
