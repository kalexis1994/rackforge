"""Diagnose the reported Campanella ticks near 49 s without changing DSP.

Render each counterfactual from the beginning with the original MIDI history.
Controls at their actual off position isolate causal effects, not additive
source stems: the downstream instrument includes nonlinear processing.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, sosfilt


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root = Path(__file__).resolve().parents[1]
    work = root/'local/concert-grand-short-attacks-2026-10-14'
    audio = Path('C:/SHARED/Piano-Ticks-Campanella-2026-10-14')
    assert not work.exists() and not audio.exists()
    work.mkdir(); audio.mkdir()
    docs = root/'docs/concert-grand-release-body-2026-10-14'
    build_path = docs/'string-tension-verification.json'
    build = json.loads(build_path.read_text(encoding='utf-8'))
    probe = Path(build['full_render']['command'][0])
    for path, sha in build['files_sha256'].items():
        assert digest(path) == sha, path
    helper = root/'tools/render-concert-grand-release-midi.py'
    spec = importlib.util.spec_from_file_location('midi', helper)
    midi = importlib.util.module_from_spec(spec); spec.loader.exec_module(midi)
    source_path = Path('C:/Users/kalex/OneDrive/Documents/piano-comparacion/09-midi/liszt-la-campanella.mid')
    events, _ = midi.midi_events(source_path)
    rate, duration = 44100, 52

    def frame(seconds):
        value = seconds*rate
        return (2*value.numerator+value.denominator)//(2*value.denominator)

    prefix = [(frame(t), *msg) for t,msg in events if float(t)<duration and msg[0]&240 != 192]
    prefix_path = work/'original-0-52s.txt'
    prefix_path.write_text(''.join(' '.join(map(str,e))+'\n' for e in prefix),encoding='utf-8',newline='\n')
    floor = -24.082401
    variants = dict(original={}, sin_accion={27:floor}, sin_release={28:floor},
                    sin_impulso={30:floor}, sin_mecanica={27:floor,28:floor,29:floor,30:floor})
    tunings = {}
    for name, values in variants.items():
        path = work/f'{name}.txt'
        path.write_text(''.join(f'fader.{index} = {value}\n' for index,value in values.items()),encoding='utf-8',newline='\n')
        tunings[name] = path
    files = [Path(__file__), helper, source_path, build_path, probe, root/'plugins/concert-grand/src/lib.rs',
             root/'plugins/concert-grand/src/math.rs', prefix_path, *tunings.values()]
    frozen = {str(p):digest(p) for p in files}

    def render(name, event_path, frames, tuning):
        path = work/f'{name}.f32'
        command = [str(probe),str(event_path),str(path),str(rate),str(frames),'256','concert',str(tuning)]
        result = json.loads(subprocess.check_output(command,cwd=root,text=True))
        x = np.fromfile(path,dtype='<f4').reshape(-1,2)
        assert len(x)==frames and np.isfinite(x).all() and abs(x).max()<1
        return x,dict(command=command,native=result,raw_sha256=digest(path))

    def wav(name,x):
        path = audio/f'{name}.wav'
        wavfile.write(path,rate,x)
        sr,y=wavfile.read(path); assert sr==rate and np.array_equal(x,y)
        return dict(path=str(path),sha256=digest(path),frames=len(x),rate=rate,export_gain=1)

    def power(x):
        return float(np.mean(x.astype(float)**2))

    def db(x):
        return float(10*np.log10(max(power(x),1e-30)))

    def window(x,t,lo,hi):
        return x[round((t+lo)*rate):round((t+hi)*rate)]

    def features(x,t):
        stages = [[0,.015],[.015,.04],[.04,.08],[.08,.14],[.14,.24]]
        bands = [[30,300],[300,1200],[1200,2000],[2000,2600],[2600,5000],[5000,12000]]
        filtered = [sosfilt(butter(4,b,fs=rate,btype='bandpass',output='sos'), x,axis=0) for b in bands]
        return dict(stages_seconds=stages,stereo_rms_db=[db(window(x,t,*s)) for s in stages],
                    bands_hz=bands,band_stage_rms_db=[[db(window(y,t,*s)) for s in stages] for y in filtered])

    with ThreadPoolExecutor(max_workers=3) as pool:
        results = dict(zip(variants,pool.map(lambda name:render('prefix-'+name,prefix_path,duration*rate,tunings[name]),variants)))
    previous = json.loads((docs/'string-tension.json').read_text(encoding='utf-8'))
    piece = next(p for p in previous['pieces'] if p['name']=='Campanella')
    prior_raw = Path(piece['B_render']['command'][2])
    assert digest(prior_raw)==piece['B_render']['raw_sha256']
    prior = np.memmap(prior_raw,dtype='<f4',mode='r').reshape(-1,2)
    assert np.array_equal(results['original'][0],prior[:duration*rate]), 'Original prefix must reproduce listened-to audio'
    crops = {name:wav(f'Pasaje-46.8-51.2-{name}',x[round(46.8*rate):round(51.2*rate)]) for name,(x,_) in results.items()}
    held, pedal, notes = {},0,[]
    for t,msg in events:
        kind=msg[0]&240
        if kind==176 and msg[1]==64:pedal=msg[2]
        if kind==144 and msg[2]:held[msg[1]]=(float(t),msg[2],pedal)
        elif kind==128 or kind==144 and not msg[2]:
            started=held.pop(msg[1],None)
            if started and 47.5<=started[0]<51.5:
                notes.append(dict(note=msg[1],onset_seconds=started[0],velocity=started[1],
                    duration_seconds=float(t)-started[0],release_byte=msg[2],pedal_on=started[2],pedal_off=pedal))
    targets = [n for n in notes if n['note']==97 and 48.8<n['onset_seconds']<49.4]
    assert len(targets)==2
    passage=[]
    for note in targets:
        t=note['onset_seconds']
        metrics={name:features(x,t) for name,(x,_) in results.items()}
        for name,(x,_) in results.items():
            if name=='original':continue
            metrics[name]['counterfactual_difference']=features(results['original'][0]-x,t)
        passage.append(dict(midi=note,metrics=metrics))

    # Fresh single notes are diagnostic controls, not stems of the full piece:
    # removing other MIDI notes changes strike serials and action history.
    single=[]
    tests=[(note,82,.026) for note in (80,96,97,98)]
    tests.extend([(97,60,.026),(97,110,.026),(97,82,.5)])
    for note,velocity,gate in tests:
        stem=f'single-{note}-{velocity}-{round(gate*1000)}ms'
        path=work/f'{stem}.txt'
        path.write_text(f'0 144 {note} {velocity}\n{round(gate*rate)} 128 {note} 0\n',encoding='utf-8',newline='\n')
        row=dict(note=note,velocity=velocity,gate_seconds=gate,variants={})
        for name in ('original','sin_accion','sin_release','sin_impulso','sin_mecanica'):
            x,result=render(stem+'-'+name,path,rate,tunings[name])
            row['variants'][name]=dict(render=result,metrics=features(x,0))
        single.append(row)
    controls = []
    for name in ('original','sin_accion','sin_release','sin_impulso','sin_mecanica'):
        parts=[]
        for note in (80,96,97,98):
            row=next(s for s in single if s['note']==note and s['velocity']==82 and s['gate_seconds']==.026)
            raw=row['variants'][name]['render']['command'][2]
            parts.extend((np.fromfile(raw,dtype='<f4').reshape(-1,2),np.zeros((rate//4,2),dtype=np.float32)))
        controls.append(wav(f'Control-Gs5-C7-Cs7-D7-{name}',np.concatenate(parts)))
    assert frozen=={str(p):digest(p) for p in files}
    report=dict(scope=__doc__,base_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
        files_sha256=frozen,source_midi_sha256=digest(source_path),variants=variants,
        original_prefix_bit_identical_to_complete_render=True,prefixes={name:r for name,(_,r) in results.items()},
        crop_source_seconds=[46.8,51.2],crops=crops,notes=sorted(notes,key=lambda n:n['onset_seconds']),
        target_notes=passage,single_controls=single,single_listening_outputs=controls,
        limitations='Counterfactual differences include downstream nonlinear response. Fresh isolated notes do not preserve whole-piece strike serials. Levels are unnormalized internal export dBFS; no perceptual or calibrated SPL claim.')
    (docs/'short-attacks.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')
    print('Diagnostic renders and report complete',flush=True)


if __name__=='__main__':main()
