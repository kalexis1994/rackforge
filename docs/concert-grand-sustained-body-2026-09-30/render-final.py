import hashlib,importlib.util,json,subprocess
from pathlib import Path
import numpy as np
import soundfile as sf

h=Path('C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge')
r=Path('local/concert-grand-coherent-unison');frozen=r/'frozen-sustain-14'
s=importlib.util.spec_from_file_location('v3',h/'tools/concert-grand-refit-v2.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
model=json.loads((frozen/'model.json').read_text());b=frozen/'renderer.exe';sha=hashlib.sha256(b.read_bytes()).hexdigest();assert sha==model['binary_sha256']
out=r/'sustain14-final';out.mkdir(exist_ok=True)
fs=44100;clips={};longs={};gain=10**(model['training_gain_db']/20)

def render(root,rate,frames,events):
 root.mkdir(parents=True,exist_ok=True)
 (root/'events.txt').write_text('\n'.join(f'{frame} {a} {c} {d}' for frame,a,c,d in events)+'\n')
 (root/'tuning.txt').write_text('\n'.join(f'{k} = {v}' for k,v in sorted(model['tuning'].items()))+'\n')
 env=m.clean_render_environment();env.update(CG_EVENT_FILE=str((root/'events.txt').resolve()),CG_TUNING=str((root/'tuning.txt').resolve()),CG_RENDER_DIR=str(root.resolve()),CG_RATE=str(rate),CG_TOTAL_FRAMES=str(frames),CG_PRESET='concert',CG_PARAMS=','.join(f'{k}={v}' for k,v in model['params'].items()),RUST_MIN_STACK='33554432')
 signature=dict(binary_sha256=sha,model_sha256=hashlib.sha256((frozen/'model.json').read_bytes()).hexdigest(),rate=rate,frames=frames,events=events)
 manifest=root/'signature.json';raw=root/'performance.f32'
 if not (manifest.exists() and json.loads(manifest.read_text())==json.loads(json.dumps(signature)) and raw.exists() and raw.stat().st_size==frames*8):
  with (root/'render.log').open('w') as log:subprocess.run([str(b.resolve()),'--exact','tests::render_reference_wavs','--ignored','--quiet'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  manifest.write_text(json.dumps(signature,indent=2))
 x=np.fromfile(raw,dtype='<f4').reshape(-1,2);assert np.isfinite(x).all()
 return x.astype(float)

for n in (60,65,67,72,79,84,91,96):
 for v in (60,84,124):
  x=render(out/f'n{n}-v{v}',fs,4*fs,[(0,144,n,v),(round(2.5*fs),128,n,64)])
  clips[n,v]=x[:,0]*gain
 x=render(out/f'long-{n}',fs,14*fs,[(0,144,n,124),(12*fs,128,n,64)])
 longs[n]=x[:,0]*gain
print('audition renders ready',flush=True)

stress=[]
for rate in (44100,48000):
 events=[]
 for i in range(48):
  start=round(i*.08*rate)
  events.append((start,176,64,(127,72,35,0)[i%4]))
  for n in (65,67,75,84,96):
   events.extend([(start,144,n,124),(start+round(.045*rate),128,n,64)])
 events.append((round(4*rate),176,64,0));events.sort(key=lambda e:e[0])
 x=render(out/f'stress-{rate}',rate,round(12*rate),events)
 level=lambda lo,hi:float(np.mean(x[round(lo*rate):round(hi*rate)]**2))
 initial=level(3,4);tail=level(10,12)
 assert tail<initial*.01,(rate,initial,tail)
 stress.append(dict(rate=rate,block_frames=256,event_count=len(events),finite=True,raw_peak=float(abs(x).max()),late_to_active_power_db=float(10*np.log10(max(tail,1e-30)/initial))))
print('stress passed',stress,flush=True)

peak=max(float(abs(x).max()) for x in list(clips.values())+list(longs.values()))
trim=min(1.,.89/peak);folder=Path('C:/SHARED/Modelo-14');folder.mkdir(exist_ok=True);gap=np.zeros(round(.4*fs));files={}
for name,notes,mode in [('agudos-intensidades',(72,79,84,91,96),'short'),('medios-intensidades',(60,65,67,72),'short'),('agudos-sostenidas-12s',(72,79,84,91,96),'long')]:
 pairs=[(n,v) for n in notes for v in (60,84,124)] if mode=='short' else [(n,124) for n in notes]
 xs=[clips[n,v] if mode=='short' else longs[n] for n,v in pairs]
 x=np.concatenate([part for clip in xs for part in (clip*trim,gap)])
 path=folder/f'Modelo-14-{name}.wav';sf.write(path,x,fs,subtype='PCM_24')
 info=dict(path=str(path.resolve()),sha256=hashlib.sha256(path.read_bytes()).hexdigest(),duration_s=len(x)/fs,rate=fs,channels=1,channel='native left',subtype='PCM_24',pairs=pairs,hold_s=2.5 if mode=='short' else 12,release_s=1.5 if mode=='short' else 2,gap_s=.4,gain_db=model['training_gain_db'],common_trim_db=float(20*np.log10(trim)),peak=float(abs(x).max()),binary_sha256=sha,model_sha256=hashlib.sha256((frozen/'model.json').read_bytes()).hexdigest(),note_off_policy='Actual MIDI note-off; high keys without dampers continue their natural decay.')
 path.with_suffix('.json').write_text(json.dumps(info,indent=2));files[name]=info
report=dict(stress=stress,files=files,global_trim_db=float(20*np.log10(trim)),production_plugin_modified=False)
(out/'verification.json').write_text(json.dumps(report,indent=2))
print('Published',list(files),'trim',report['global_trim_db'],flush=True)
