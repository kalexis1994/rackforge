"""Diagnose the corrected user report: Nocturne, around 49 seconds.

This is independent of the Campanella finding. Use original MIDI history,
the listened-to production build, and mechanical-control counterfactuals.
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


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root=Path(__file__).resolve().parents[1]
    docs=root/'docs/concert-grand-release-body-2026-10-14'
    work=root/'local/concert-grand-nocturne-attacks-2026-10-14'
    audio=Path('C:/SHARED/Piano-Ticks-Nocturno-2026-10-14')
    assert not work.exists() and not audio.exists()
    work.mkdir();audio.mkdir()
    build_path=docs/'string-tension-verification.json'
    build=json.loads(build_path.read_text(encoding='utf-8'))
    for path,h in build['files_sha256'].items():assert sha(path)==h,path
    probe=Path(build['full_render']['command'][0])
    helper=root/'tools/render-concert-grand-release-midi.py'
    spec=importlib.util.spec_from_file_location('midi',helper)
    midi=importlib.util.module_from_spec(spec);spec.loader.exec_module(midi)
    source=Path('C:/Users/kalex/OneDrive/Documents/piano-comparacion/09-midi/chopin-nocturno-op9-2.mid')
    events,_=midi.midi_events(source)
    rate=44100;duration=53
    def frame(t):
        n=t*rate
        return (2*n.numerator+n.denominator)//(2*n.denominator)
    prefix=[(frame(t),*message) for t,message in events if float(t)<duration and message[0]&240!=192]
    event_path=work/'original-0-53s.txt'
    event_path.write_text(''.join(' '.join(map(str,e))+'\n' for e in prefix),encoding='utf-8',newline='\n')
    floor=-24.082401
    variants=dict(original={},sin_accion={27:floor},sin_release={28:floor},sin_impulso={30:floor},sin_mecanica={27:floor,28:floor,29:floor,30:floor})
    tunings={}
    for name,values in variants.items():
        p=work/f'{name}.txt';p.write_text(''.join(f'fader.{i} = {v}\n' for i,v in values.items()),encoding='utf-8',newline='\n');tunings[name]=p
    files=[Path(__file__),helper,source,build_path,probe,event_path,root/'plugins/concert-grand/src/lib.rs',*tunings.values()]
    frozen={str(p):sha(p) for p in files}
    def render(name):
        raw=work/f'{name}.f32'
        command=[str(probe),str(event_path),str(raw),str(rate),str(duration*rate),'256','concert',str(tunings[name])]
        native=json.loads(subprocess.check_output(command,cwd=root,text=True))
        x=np.fromfile(raw,dtype='<f4').reshape(-1,2)
        assert len(x)==duration*rate and np.isfinite(x).all() and abs(x).max()<1
        return x,dict(command=command,native=native,raw_sha256=sha(raw))
    with ThreadPoolExecutor(max_workers=3) as pool:results=dict(zip(variants,pool.map(render,variants)))
    accepted=json.loads((docs/'string-tension.json').read_text(encoding='utf-8'))
    piece=next(p for p in accepted['pieces'] if p['name']=='Nocturno')
    original=Path(piece['B_render']['command'][2]);assert sha(original)==piece['B_render']['raw_sha256']
    previous=np.memmap(original,dtype='<f4',mode='r').reshape(-1,2)
    assert np.array_equal(results['original'][0],previous[:duration*rate])
    held={};pedal=0;notes=[]
    for t,m in events:
        kind=m[0]&240
        if kind==176 and m[1]==64:pedal=m[2]
        if kind==144 and m[2]:held[m[1]]=(float(t),m[2],pedal)
        elif kind==128 or kind==144 and not m[2]:
            n=held.pop(m[1],None)
            if n and 47.5<=n[0]<51.6:
                notes.append(dict(note=m[1],onset_seconds=n[0],velocity=n[1],gate_seconds=float(t)-n[0],off_seconds=float(t),release_byte=m[2],pedal_on=n[2],pedal_off=pedal))
    notes.sort(key=lambda n:n['onset_seconds'])
    windows=[[0,.012],[.012,.035],[.035,.07],[.07,.12],[.12,.2]]
    bands=[[30,300],[300,1200],[1200,3000],[3000,10000]]
    def level(x):return float(10*np.log10(max(float(np.mean(x.astype(float)**2)),1e-30)))
    def metrics(x,n):
        t=n['onset_seconds']
        # Include 200 ms of history to settle the diagnostic band filters.
        base=round((t-.2)*rate);y=x[base:round((t+.25)*rate)]
        layers=[y]+[sosfilt(butter(4,b,fs=rate,btype='bandpass',output='sos'),y,axis=0) for b in bands]
        return dict(stereo_rms_db=[level(x[round((t+a)*rate):round((t+b)*rate)]) for a,b in windows],
                    band_stage_rms_db=[[level(z[round((a+.2)*rate):round((b+.2)*rate)]) for a,b in windows] for z in layers[1:]])
    for note in notes:
        note['metrics']={name:metrics(x,note) for name,(x,_) in results.items()}
        for name,(x,_) in results.items():
            if name!='original':note['metrics'][name]['counterfactual_difference']=metrics(results['original'][0]-x,note)
    def wav(name,x):
        p=audio/f'{name}.wav';assert not p.exists();wavfile.write(p,rate,x)
        sr,y=wavfile.read(p);assert sr==rate and np.array_equal(x,y)
        return dict(path=str(p),sha256=sha(p),rate=rate,export_gain=1)
    crops={}
    for name,(x,_) in results.items():
        crops[name]=wav(f'Nocturno-47.5-51.6-{name}',x[round(47.5*rate):round(51.6*rate)])
    assert frozen=={str(p):sha(p) for p in files}
    report=dict(scope=__doc__,base_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
        files_sha256=frozen,original_prefix_bit_identical_to_complete_render=True,
        source_midi_sha256=sha(source),variants=variants,prefixes={name:r for name,(_,r) in results.items()},
        windows_relative_seconds=windows,bands_hz=bands,notes=notes,crop_source_seconds=[47.5,51.6],crops=crops,
        limitations='Counterfactuals affect all notes and downstream nonlinear response; overlapping notes prevent source separation from window RMS alone. No synthesis changes or Campanella diagnosis assumed.')
    (docs/'nocturne-attacks.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')
    for n in notes:
        a=np.array(n['metrics']['original']['stereo_rms_db'])
        print(n['note'],round(n['onset_seconds'],6),'v',n['velocity'],'gate',round(1000*n['gate_seconds'],2),
              'rms',np.round(a,2),'off_action',np.round(np.array(n['metrics']['sin_accion']['stereo_rms_db'])-a,2),
              'off_release',np.round(np.array(n['metrics']['sin_release']['stereo_rms_db'])-a,2),flush=True)


if __name__=='__main__':main()
