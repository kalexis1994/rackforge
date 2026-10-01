"""Render frozen research unison profiles, with 12-second holds and real releases.

Uses one fresh renderer process per long note to avoid shared-tail contamination.
Every audition uses one common gain, without normalizing individual notes/profiles.
"""
import argparse, hashlib, importlib.util, json, subprocess
from pathlib import Path
import numpy as np
import soundfile as sf
from scipy.signal import find_peaks, resample_poly


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--research-tools',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--audition-folder',type=Path,required=True)
    p.add_argument('--profiles',type=Path,help='Ordered JSON mapping of profile names to overrides of --model tuning.')
    p.add_argument('--experiment-label',default='PUENTE')
    p.add_argument('--notes', default='60,65,67,72', help='Comma-separated MIDI notes in playback order.')
    p.add_argument('--long-velocity', type=int, default=84)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    a.audition_folder.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('settling',a.research_tools/'audit-concert-grand-settling.py')
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
    model=json.loads(a.model.read_text());base=model['tuning'];fs=44100
    profiles={'07':base,'09':{**base,'COHERENT_UNISON':1,'UNISON_WIDTH':.5,'STRING_STIFFNESS_SPREAD':.02},
        '10':{**base,'COHERENT_UNISON':1,'UNISON_WIDTH':.5,'STRING_STIFFNESS_SPREAD':.02,'REACTIVE_BRIDGE':1}}
    if a.profiles:
        requested=json.loads(a.profiles.read_text())
        profiles={name:{**base,**(overrides.get('tuning',{}) if 'params' in overrides or 'tuning' in overrides else overrides)} for name,overrides in requested.items()}
        profile_params={name:{**model['params'],**overrides.get('params',{})} for name,overrides in requested.items()}
    else:
        profile_params={name:model['params'] for name in profiles}
    if not profiles:raise ValueError('at least one profile is required')
    baseline_name=next(iter(profiles))
    notes=tuple(int(n) for n in a.notes.split(','));velocities=(60,84,124)
    if not notes or any(n<21 or n>108 for n in notes) or not 1<=a.long_velocity<=127:
        raise ValueError('notes must be 21..108 and velocity 1..127')
    long_velocity=a.long_velocity
    sha=hashlib.sha256(a.binary.read_bytes()).hexdigest()
    gain=10**(model['training_gain_db']/20)
    long_clips={};short_clips={};audit={};peak=0.0
    for name,tuning in profiles.items():
        root=a.output/name
        params=profile_params[name]
        m.b.v3.render_candidate(a.binary,root,[(n,v) for n in notes for v in velocities],params,tuning=tuning,warmup_ms=200)
        short_clips[name]=[];long_clips[name]=[];audit[name]={}
        for n in notes:
            for v in velocities:
                x,rate=sf.read(root/f'model{n:03}v{v}.wav');assert rate==fs and x.ndim==1
                clip=x[:4*fs]*gain;short_clips[name].append(clip);peak=max(peak,float(abs(clip).max()))
            directory=root/f'long-{n}';directory.mkdir(exist_ok=True)
            event=directory/'events.txt';event.write_text(f'0 144 {n} {long_velocity}\n{12*fs} 128 {n} 64\n')
            tuning_path=directory/'tuning.txt';tuning_path.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning.items()))+'\n')
            signature=dict(binary_sha256=sha,tuning=tuning,note=n,velocity=long_velocity,hold_s=12,release_s=2,rate=fs,params=params)
            manifest=directory/'manifest.json';raw=directory/'performance.f32'
            if not (manifest.exists() and json.loads(manifest.read_text())==signature and raw.exists() and raw.stat().st_size==14*fs*2*4):
                env=m.b.v3.clean_render_environment()
                env.update(CG_EVENT_FILE=str(event.resolve()),CG_TOTAL_FRAMES=str(14*fs),CG_RATE=str(fs),CG_TUNING=str(tuning_path.resolve()),
                    CG_RENDER_DIR=str(directory.resolve()),CG_PRESET='concert',CG_PARAMS=','.join(f'{k}={v}' for k,v in params.items()),RUST_MIN_STACK='33554432')
                with (directory/'render.log').open('w') as log:
                    subprocess.run([str(a.binary.resolve()),'--exact','tests::render_reference_wavs','--ignored','--quiet'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
                manifest.write_text(json.dumps(signature,indent=2))
            x=np.fromfile(raw,dtype='<f4').reshape(-1,2)[:,0].astype(float)*gain
            assert np.isfinite(x).all();peak=max(peak,float(abs(x).max()));long_clips[name].append(x)
            low=resample_poly(x[:12*fs],1,4);features=m.b.features(root/f'model{n:03}v{long_velocity}.wav',n)
            if name==baseline_name:
                ceiling=max(f['level_db'] for f in features)
                audit[name][str(n)]={'mask':[f['level_db']>ceiling-35 for f in features]}
            usable=audit[baseline_name][str(n)]['mask'];partials=[]
            for f,allowed in zip(features,usable):
                if not allowed:continue
                envelope=m.envelope(low,f['frequency_hz']);times=np.arange(len(envelope))/m.b.ENV_RATE
                minima,props=find_peaks(-envelope,prominence=3,distance=round(.12*m.b.ENV_RATE))
                floor=max(float(envelope[(times>=.1)&(times<.5)].max())-40,-90)
                events=[dict(time_s=float(times[t]),depth_db=float(props['prominences'][i])) for i,t in enumerate(minima)
                    if .1<times[t]<11.5 and envelope[t]>floor]
                partials.append(dict(harmonic=f['harmonic'],frequency_hz=f['frequency_hz'],notches=events))
            row=dict(partials=partials,
                intervals=[dict(start_s=lo,end_s=hi,count=sum(lo<=e['time_s']<hi for r in partials for e in r['notches']))
                    for lo,hi in ((.1,.5),(.5,1.5),(1.5,3.5),(3.5,7.5),(7.5,11.5))],
                levels_db=[float(10*np.log10(max(np.mean(x[round(lo*fs):round(hi*fs)]**2),1e-24)))
                    for lo,hi in ((.05,.2),(.5,1.5),(1.5,3.5),(3.5,7.5),(7.5,11.5),(12,14))])
            if name==baseline_name:row['mask']=usable
            audit[name][str(n)]=row
        print('rendered',name,flush=True)
    trim=min(1.,.89/peak);gap=np.zeros(round(.4*fs))
    exported={}
    for name in profiles:
        for kind,groups in (('intensidades',short_clips),('sostenidas-12s',long_clips)):
            path=a.audition_folder/f'{name}-{a.experiment_label}-{kind}.wav'
            audio=np.concatenate([item for clip in groups[name] for item in (clip*trim,gap)])
            sf.write(path,audio,fs,subtype='PCM_24')
            exported[str(path)]=dict(sha256=hashlib.sha256(path.read_bytes()).hexdigest(),duration_s=len(audio)/fs)
    result=dict(profiles=profiles,params=model['params'],params_by_profile=profile_params,binary_sha256=sha,model_sha256=hashlib.sha256(a.model.read_bytes()).hexdigest(),
        notes=list(notes),velocities=list(velocities),long_velocity=long_velocity,gain_db=model['training_gain_db'],common_trim_db=float(20*np.log10(trim)),
        policy=f'Exploratory mono left-channel comparison; common {baseline_name} partial mask. Counts are diagnostics, not independent audible defects. No original F4/G4 comparison.',audit=audit,files=exported)
    (a.output/'audition.json').write_text(json.dumps(result,indent=2))
    (a.audition_folder/f'{a.experiment_label}-experimento.json').write_text(json.dumps(result,indent=2))
    legend=f'Perfiles: {", ".join(profiles)}. Referencia de esta prueba: {baseline_name}. Valores exactos en el JSON.\n'
    note_names=', '.join(f'{("C","C#","D","D#","E","F","F#","G","G#","A","A#","B")[n%12]}{n//12-1}' for n in notes)
    instructions=legend+f'''EXPERIMENTOS: candidatos para escuchar; parametros de investigacion.
Intensidades: {note_names}; cada nota a velocidades 60, 84, 124.
Cada fragmento dura 4 segundos sostenidos + 0.4 segundos de silencio. No es una liberacion de tecla.
Sostenidas: {note_names}, velocidad {long_velocity}.
Cada nota se mantiene 12 segundos, se suelta y se conserva 2 segundos de release, luego 0.4 segundos de silencio.
Misma ganancia y trim para todos los archivos; no hay normalizacion por nota ni por variante.
'''
    (a.audition_folder/f'{a.experiment_label}-LEEME.txt').write_text(instructions,encoding='utf-8')
    print(json.dumps({name:[sum(row['intervals'][i]['count'] for row in rows.values()) for i in range(5)] for name,rows in audit.items()}),flush=True)


if __name__=='__main__':main()
