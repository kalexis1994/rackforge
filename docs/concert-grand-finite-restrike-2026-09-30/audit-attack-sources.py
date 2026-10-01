"""Paired source ablations on an exact MIDI event stream, including its preroll.

The differences diagnose synthetic sources, not the sound of a real mechanism.
All exports use one supplied gain; no peak normalization or post EQ.
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
from scipy.signal import butter, sosfilt


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools', type=Path, required=True)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--model', type=Path, required=True)
    p.add_argument('--events', type=Path, required=True)
    p.add_argument('--baseline', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--clip-start', type=float, default=143)
    p.add_argument('--clip-end', type=float, default=153)
    p.add_argument('--gain-db', type=float, required=True)
    p.add_argument('--profiles', type=Path)
    a = p.parse_args()
    rate = 44100
    a.output.mkdir(parents=True, exist_ok=True)
    model = json.loads(a.model.read_text(encoding='utf-8'))
    assert sha(a.binary) == model['binary_sha256']
    spec = importlib.util.spec_from_file_location('refit', a.research_tools/'concert-grand-refit-v2.py')
    refit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(refit)
    profiles = json.loads(a.profiles.read_text(encoding='utf-8')) if a.profiles else {
        'sin-ruido-mecanico': {'KNOCK_LEVEL': 0, 'THUMP_BASE': 0},
        'sin-pulso-contacto': {'CONTACT_BOARD_GAIN': 0},
        'repeticion-contacto': {'MERGE_RAMP_CONTACT': 1},
    }
    total = round((a.clip_end+1)*rate)
    start, end = round(a.clip_start*rate), round(a.clip_end*rate)
    events = [line for line in a.events.read_text().splitlines() if int(line.split()[0]) < total]
    event_text = '\n'.join(events)+'\n'

    def render(item):
        name, override = item
        folder = a.output/name
        folder.mkdir(exist_ok=True)
        (folder/'events.txt').write_text(event_text, encoding='utf-8')
        tuning = {**model['tuning'], **override}
        (folder/'tuning.txt').write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning.items()))+'\n', encoding='utf-8')
        signature = dict(binary=sha(a.binary), model=sha(a.model), tuning=tuning,
                         params=model['params'], events=sha(folder/'events.txt'), frames=total)
        raw_path = folder/'performance.f32'
        manifest = folder/'manifest.json'
        cached = manifest.exists() and json.loads(manifest.read_text()) == signature
        if not (cached and raw_path.exists() and raw_path.stat().st_size == total*8):
            env = refit.clean_render_environment()
            env.update(CG_EVENT_FILE=str((folder/'events.txt').resolve()), CG_TOTAL_FRAMES=str(total),
                       CG_RENDER_DIR=str(folder.resolve()), CG_TUNING=str((folder/'tuning.txt').resolve()),
                       CG_PRESET='concert', CG_PARAMS=','.join(f'{k}={v}' for k,v in model['params'].items()),
                       CG_RATE=str(rate), RUST_MIN_STACK='33554432')
            with (folder/'render.log').open('w', encoding='utf-8') as log:
                subprocess.run([str(a.binary.resolve()), 'tests::render_reference_wavs', '--exact',
                                '--ignored', '--nocapture', '--test-threads=1'], env=env,
                               stdout=log, stderr=subprocess.STDOUT, check=True)
            assert 'complaints: []' in (folder/'render.log').read_text()
            manifest.write_text(json.dumps(signature, indent=2), encoding='utf-8')
        print('rendered', name, flush=True)
        return name, raw_path

    with ThreadPoolExecutor(max_workers=2) as pool:
        paths = {'14-original': a.baseline, **dict(pool.map(render, profiles.items()))}
    sos = butter(4, 8000, fs=rate, btype='highpass', output='sos')
    onsets = []
    previous = {}
    for line in events:
        fields = list(map(int,line.split()))
        at, status = fields[:2]
        if status & 0xf0 == 0x90 and len(fields) == 4 and fields[3]:
            note, velocity = fields[2:]
            key = (status & 0xf, note)
            interval = (at-previous[key])/rate if key in previous else None
            previous[key] = at
            if start <= at < end:
                onsets.append(dict(frame=at, seconds=at/rate, note=note, velocity=velocity,
                                   previous_same_note_seconds=interval))
    signals = {}
    results = {}
    gain = 10**(a.gain_db/20)
    for name, path in paths.items():
        raw = np.memmap(path, dtype='<f4', mode='r').reshape(-1,2)
        assert len(raw) >= total and np.isfinite(raw[:total]).all()
        clip = raw[start:end].astype(np.float64)
        assert abs(clip*gain).max() < 1, 'Common gain would clip'
        sf.write(a.output/(name+'.wav'), clip*gain, rate, subtype='PCM_24')
        signals[name] = clip
        mono = raw[:total].mean(axis=1).astype(np.float64)
        hf = sosfilt(sos, mono)
        rows = []
        for onset in onsets:
            t = onset['frame']
            early = np.mean(hf[t:t+round(.002*rate)]**2)
            body = np.mean(hf[t+round(.004*rate):t+round(.012*rate)]**2)
            full = np.mean(mono[t:t+round(.020*rate)]**2)
            rows.append({**onset, 'hf_0_2ms_power': float(early),
                         'hf_crest_db': float(10*np.log10(max(early,1e-30)/max(body,1e-30))),
                         'attack_0_20ms_power': float(full)})
        results[name] = dict(raw_sha256=sha(path), rows=rows,
                            clip_peak=float(abs(clip*gain).max()),
                            hf_attack_power=float(sum(r['hf_0_2ms_power'] for r in rows)),
                            median_hf_crest_db=float(np.median([r['hf_crest_db'] for r in rows])))
    baseline = signals['14-original']
    for name in profiles:
        delta = baseline-signals[name]
        sf.write(a.output/(name+'-diferencia.wav'), delta*gain, rate, subtype='PCM_24')
        results[name]['source_difference_rms_db_relative_total'] = float(
            10*np.log10(max(float(np.mean(delta**2)),1e-30)/max(float(np.mean(baseline**2)),1e-30)))
    report = dict(model_sha256=sha(a.model), binary_sha256=sha(a.binary),
                  original_events_sha256=sha(a.events), clip_seconds=[a.clip_start,a.clip_end],
                  gain_db=a.gain_db, profiles=profiles, results=results,
                  policy='Identical MIDI from frame zero, including pedal and voices before the excerpt; paired counterfactuals; same gain. HF onsets are diagnostic, not an audibility score.')
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({name:{k:v for k,v in info.items() if k not in ('rows','raw_sha256')}
                      for name,info in results.items()},indent=2),flush=True)
    print(json.dumps(onsets,indent=2),flush=True)


if __name__ == '__main__':
    main()
