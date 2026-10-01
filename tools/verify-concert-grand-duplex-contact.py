"""Check the residual 97.40755s click and preservation of six fresh strikes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfiltfilt


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('before-model', 'after-model', 'binary', 'before-grid',
                 'before-midi', 'after-midi', 'work', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    a = p.parse_args()
    before = json.loads(a.before_model.read_text(encoding='utf-8'))
    after = json.loads(a.after_model.read_text(encoding='utf-8'))
    tuning = {**after['model']['tuning'], **after['overrides']}
    assert tuning == {**before['model']['tuning'], **before['overrides']}
    assert before['model']['params'] == after['model']['params']
    assert sha(a.binary) == after['model']['binary_sha256']
    a.work.mkdir(parents=True, exist_ok=True)
    grid = a.work/'grid'
    grid.mkdir(exist_ok=True)
    config = a.work/'tuning.txt'
    config.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning.items()))+'\n')
    env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
    env.update(CG_TUNING=str(config.resolve()), CG_GRID='36:124,60:60,60:124,72:124,90:124,96:124',
               CG_RENDER_DIR=str(grid.resolve()), CG_PARAMS=','.join(
                   f'{k}={v}' for k,v in after['model']['params'].items()),
               CG_PRESET='concert', CG_RATE='44100', CG_WARMUP_MS='200', CG_LEFT='1',
               RUST_MIN_STACK='33554432')
    with (a.work/'render.log').open('w') as log:
        subprocess.run([str(a.binary.resolve()), '--exact', 'tests::render_reference_wavs',
                        '--ignored', '--nocapture', '--test-threads=1'], env=env,
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    assert 'complaints: []' in (a.work/'render.log').read_text()
    fresh = {}
    for wav in grid.glob('*.wav'):
        x, rate = sf.read(wav)
        y, old_rate = sf.read(a.before_grid/wav.name)
        assert rate == old_rate == 44100 and np.array_equal(x,y), wav.name
        fresh[wav.name] = dict(samples_equal=True, sha256=sha(wav))
    assert len(fresh) == 6
    a.output.mkdir(parents=True, exist_ok=True)
    gain_db = 25.120884443707872
    gain = 10**(gain_db/20)
    signals = {}
    metrics = {}
    rate = 44100
    target = 4295673
    sos = butter(4, (8000,18000), fs=rate, btype='bandpass', output='sos')
    for label,folder in [('Antes',a.before_midi), ('Corregido',a.after_midi)]:
        raw = np.memmap(folder/'liszt-la-campanella/performance.f32',dtype='<f4',mode='r').reshape(-1,2)
        assert np.isfinite(raw).all()
        signals[label] = raw
        local = raw[target-2205:target+2205].astype(np.float64)
        high = sosfiltfilt(sos,local,axis=0)
        metrics[label] = dict(hf_first_ms_power=float(np.mean(high[2205:2249]**2)),
                             hf_first_3ms_power=float(np.mean(high[2205:2337]**2)))
        for stem, start,end in [('Campanella-92s-100s',92,100),('Campanella-36s-44s',36,44)]:
            clip = raw[start*rate:end*rate].astype(np.float64)*gain
            assert abs(clip).max() < 1
            sf.write(a.output/f'{stem}-{label}.wav',clip,rate,subtype='PCM_24')
        noct = np.memmap(folder/'chopin-nocturno-op9-2/performance.f32',dtype='<f4',mode='r').reshape(-1,2)
        sf.write(a.output/f'Nocturno-2s-10s-{label}.wav',noct[2*rate:10*rate].astype(float)*gain,
                 rate,subtype='PCM_24')
    reduction = 10*np.log10(metrics['Corregido']['hf_first_ms_power']/metrics['Antes']['hf_first_ms_power'])
    assert reduction < -20, f'Initial high-frequency spike remains: {reduction} dB'
    # Causal engine: before the first duplex restrike the complete piece
    # must still be sample-identical. This also guards accidental voicing.
    frames = signals['Antes'].shape[0]
    assert signals['Corregido'].shape[0] == frames
    first_difference = None
    for start in range(0,frames,rate*10):
        diff = np.any(signals['Antes'][start:start+rate*10] != signals['Corregido'][start:start+rate*10],axis=1)
        if np.any(diff):
            first_difference = start+int(np.flatnonzero(diff)[0])
            break
    assert first_difference is not None
    midi_checks = {}
    for name in ('Fieltro-experimental-La-Campanella','Fieltro-experimental-Nocturno-op9-2'):
        old = json.loads((a.before_model.parent/'MIDI'/f'{name}.json').read_text(encoding='utf-8'))
        new = json.loads((a.after_model.parent/'MIDI'/f'{name}.json').read_text(encoding='utf-8'))
        keys = ('midi_sha256','events_sha256','total_frames','channel_messages','note_attacks',
                'controller_values','gain_db','sample_rate','channels')
        checks = {key:old[key] == new[key] for key in keys}
        assert all(checks.values()), (name,checks)
        midi_checks[name] = checks
    report = dict(policy=__doc__, before_model_sha256=sha(a.before_model), after_model_sha256=sha(a.after_model),
                  binary_sha256=sha(a.binary), tuning_identical=True, fresh_strikes=fresh,
                  target_frame=target,target_seconds=target/rate,target_note=95,target_velocity=71,
                  metrics=metrics,first_ms_hf_change_db=float(reduction),
                  first_complete_piece_difference_frame=first_difference,gain_db=gain_db,midi_checks=midi_checks,
                  limitation='The metric isolates this reported onset; listening determines remaining audible clicks.')
    (a.output/'validacion.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    print(f'PASS: six fresh strikes identical; target HF first millisecond {reduction:.2f} dB.')


if __name__ == '__main__':
    main()
