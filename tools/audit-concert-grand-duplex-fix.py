"""Verify the factory sound equals the listener-selected duplex ablation.

Compare immutable native renderers with the full MIDI preroll and shared
unity export gain. This measures the change; listening determines whether
other clicks remain in the musical passage.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt


RATE = 44100


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def render(binary, events, frames, folder, tuning, preset='concert', params=None):
    folder.mkdir(parents=True, exist_ok=True)
    event_file, tuning_file = folder/'events.txt', folder/'tuning.txt'
    event_file.write_text(events, encoding='utf-8')
    tuning_file.write_text('\n'.join(f'{k}={v}' for k,v in sorted(tuning.items()))+'\n', encoding='utf-8')
    signature = dict(binary_sha256=sha(binary), events_sha256=sha(event_file),
                     frames=frames, rate=RATE, preset=preset, tuning=tuning,
                     warmup_ms=200, block_frames=256)
    if params:
        signature['params'] = params
    manifest, raw = folder/'manifest.json', folder/'performance.f32'
    if not (manifest.exists() and json.loads(manifest.read_text()) == signature
            and raw.exists() and raw.stat().st_size == frames*8):
        env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event_file.resolve()), CG_TOTAL_FRAMES=str(frames),
                   CG_RENDER_DIR=str(folder.resolve()), CG_TUNING=str(tuning_file.resolve()),
                   CG_PRESET=preset, CG_RATE=str(RATE), CG_WARMUP_MS='200',
                   RUST_MIN_STACK='33554432')
        if params:
            env['CG_PARAMS'] = ','.join(f'{k}={v}' for k,v in params.items())
        with (folder/'render.log').open('w', encoding='utf-8') as log:
            subprocess.run([str(binary.resolve()), 'tests::render_reference_wavs', '--exact',
                            '--ignored', '--nocapture', '--test-threads=1'], env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        assert 'complaints: []' in (folder/'render.log').read_text()
        manifest.write_text(json.dumps(signature, indent=2), encoding='utf-8')
    return raw


def load(path):
    return np.memmap(path, dtype='<f4', mode='r').reshape(-1,2)


def write(path, samples):
    assert np.isfinite(samples).all() and abs(samples).max() < 1
    sf.write(path, samples, RATE, subtype='PCM_24')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['baseline-binary', 'candidate-binary', 'baseline-midi', 'events',
                 'probe-events', 'baseline-probe', 'selected-probe', 'work', 'output']:
        p.add_argument('--'+name, type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    frames = 116*RATE
    prefix = '\n'.join(line for line in a.events.read_text().splitlines()
                       if int(line.split()[0]) < frames)+'\n'
    jobs = [
        (a.candidate_binary, a.probe_events.read_text(), 14*RATE, a.work/'notes', {}),
        (a.baseline_binary, prefix, frames, a.work/'midi-ablation', {'DUPLEX_LEVEL':0}),
        (a.candidate_binary, prefix, frames, a.work/'midi-candidate', {}),
    ]
    with ThreadPoolExecutor(max_workers=2) as pool:
        candidate_probe, selected_midi, candidate_midi = list(pool.map(lambda job: render(*job), jobs))
    notes = load(candidate_probe)
    selected_notes = load(a.selected_probe)
    assert len(notes) >= len(selected_notes)
    probe_matches = np.array_equal(notes[:len(selected_notes)], selected_notes)
    midi_matches = np.array_equal(load(candidate_midi), load(selected_midi))
    assert probe_matches, 'Factory notes differ from the listener-selected C ablation'
    assert midi_matches, 'Factory MIDI differs from the frozen renderer with DUPLEX_LEVEL=0'
    baseline_notes = load(a.baseline_probe)
    write(a.output/'Notas-cortas-sin-duplex.wav', notes)
    clips = [x[8*RATE:round(9.3*RATE)].astype(float) for x in [baseline_notes, notes]]
    for name, x in zip(['Csharp6-antes.wav','Csharp6-corregido.wav'], clips):
        write(a.output/name, x)
    write(a.output/'Csharp6-antes-despues.wav', np.concatenate([clips[0],np.zeros((round(.35*RATE),2)),clips[1]]))
    metrics={}
    for name, x in zip(['before','after'], clips):
        metrics[name]={'peak':float(abs(x).max())}
        onset=round(.3*RATE)
        for lo,hi in [(20,250),(1000,4000),(4000,8000)]:
            filtered=sosfilt(butter(4,[lo,hi],fs=RATE,btype='bandpass',output='sos'),x,axis=0)
            metrics[name][f'rms_{lo}_{hi}_first_15ms']=float(np.sqrt(np.mean(filtered[onset:onset+round(.015*RATE)]**2)))
    musical=[load(path)[106*RATE:115*RATE].astype(float) for path in [a.baseline_midi,candidate_midi]]
    assert all(len(x)==9*RATE for x in musical)
    for name,x in zip(['Campanella-antes-106s-115s.wav','Campanella-corregida-106s-115s.wav'],musical):
        write(a.output/name,x)
    write(a.output/'Campanella-antes-despues.wav',np.concatenate([musical[0],np.zeros((RATE//2,2)),musical[1]]))
    report=dict(baseline_binary_sha256=sha(a.baseline_binary),candidate_binary_sha256=sha(a.candidate_binary),
                events_sha256=sha(a.events),probe_events_sha256=sha(a.probe_events),
                candidate_probe_sha256=sha(candidate_probe),selected_probe_sha256=sha(a.selected_probe),
                candidate_midi_sha256=sha(candidate_midi),selected_midi_sha256=sha(selected_midi),
                baseline_midi_sha256=sha(a.baseline_midi),
                probe_bit_exact_to_selected_C=probe_matches,midi_bit_exact_to_duplex_ablation=midi_matches,
                probe_frames_compared=len(selected_notes),midi_frames_compared=frames,
                csharp6_first_attack=metrics,sample_rate=RATE,export_gain_db=0,
                musical_clip_seconds=[106,115],
                musical_peak_before=float(abs(musical[0]).max()),musical_peak_after=float(abs(musical[1]).max()),
                policy='Full MIDI preroll and unchanged pedals. Common unity export gain. Only the direct synthetic duplex excitation is removed relative to the frozen renderer.')
    (a.output/'mediciones.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'Corrección seleccionada: la C no tiene el tick. DUPLEX_LEVEL=0 en todos los programas.\n'
        'Csharp6-antes-despues: tres notas antes, pausa, las mismas tres corregidas.\n'
        'Notas-cortas-sin-duplex: siete grupos; C#6 es el quinto, a 8.3 s.\n'
        'Campanella: pasaje original 1:46 a 1:55, con todo el MIDI previo y pedales.\n'
        'Ganancia idéntica, sin normalización ni EQ. Tres cuerdas principales conservadas.\n'
        'Esta propuesta no está empaquetada ni instalada. La Campanella debe evaluarse por oído.\n',encoding='utf-8')
    print(json.dumps(report,indent=2),flush=True)


if __name__ == '__main__':
    main()
