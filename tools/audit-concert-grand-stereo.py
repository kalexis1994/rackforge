"""Audition the existing width control, preserving the accepted bass/contact model."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter,sosfiltfilt


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def stats(x):
    bands=(None,(40,180),(180,600),(600,1800),(1800,8000))
    result={}
    for band in bands:
        y=x if band is None else sosfiltfilt(butter(4,band,fs=44100,btype='bandpass',output='sos'),x,axis=0)
        mid=(y[:,0]+y[:,1])*.5;side=(y[:,0]-y[:,1])*.5
        power=lambda z:float(np.mean(z*z))
        result[str(band)]=dict(correlation=float(np.corrcoef(y.T)[0,1]),
            side_mid_db=float(10*np.log10(max(power(side),1e-30)/max(power(mid),1e-30))),
            mono_power_db=float(10*np.log10(max(power(mid),1e-30))),
            stereo_power_db=float(10*np.log10(max(power(y),1e-30))))
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('model','binary','baseline','events','work','output'):
        p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args();a.work.mkdir(parents=True,exist_ok=True);a.output.mkdir(parents=True,exist_ok=True)
    candidate=json.loads(a.model.read_text(encoding='utf-8'));model=candidate['model']
    tuning={**model['tuning'],**candidate['overrides']}
    assert sha(a.binary)==model['binary_sha256']
    variants={'Apertura-70':.7,'Apertura-100':1.0}
    passages={'liszt-la-campanella':((36,44),(92,100)), 'chopin-nocturno-op9-2':((2,10),)}
    gain_db=25.120884443707872;gain=10**(gain_db/20)
    def render(item):
        name,stem=item;folder=a.work/name/stem;folder.mkdir(parents=True,exist_ok=True)
        params={**model['params'],'4':variants[name]}
        total=(max(end for start,end in passages[stem])+1)*44100
        source=a.events/stem/'events.txt'
        text='\n'.join(line for line in source.read_text().splitlines() if int(line.split()[0])<total)+'\n'
        event=folder/'events.txt';event.write_text(text,encoding='utf-8')
        config=folder/'tuning.txt';config.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(tuning.items()))+'\n')
        signature=dict(binary_sha256=sha(a.binary),params=params,tuning=tuning,frames=total,
                       events_sha256=sha(event),source_events_sha256=sha(source))
        raw=folder/'performance.f32';marker=folder/'manifest.json'
        if not (marker.exists() and json.loads(marker.read_text())==signature and raw.exists() and raw.stat().st_size==total*8):
            env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
            env.update(CG_EVENT_FILE=str(event.resolve()),CG_TOTAL_FRAMES=str(total),CG_RENDER_DIR=str(folder.resolve()),
                       CG_TUNING=str(config.resolve()),CG_PARAMS=','.join(f'{k}={v}' for k,v in params.items()),
                       CG_RATE='44100',CG_PRESET='concert',RUST_MIN_STACK='33554432')
            with (folder/'render.log').open('w') as log:
                subprocess.run([str(a.binary.resolve()),'--exact','tests::render_reference_wavs','--ignored',
                                '--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            assert 'complaints: []' in (folder/'render.log').read_text()
            marker.write_text(json.dumps(signature,indent=2),encoding='utf-8')
        assert raw.stat().st_size==total*8
        x=np.memmap(raw,dtype='<f4',mode='r').reshape(-1,2)
        assert np.isfinite(x).all()
        print('Rendered',name,stem,flush=True)
        return (name,stem),raw
    with ThreadPoolExecutor(max_workers=2) as pool:
        paths=dict(pool.map(render,[(name,stem) for name in variants for stem in passages]))
    report=dict(model_sha256=sha(a.model),binary_sha256=sha(a.binary),gain_db=gain_db,
                previous_width=.35,variants=variants,tuning=tuning,metrics={},
                policy=__doc__+' Native rendering from MIDI frame zero. Fixed gain, no postprocessing stereo enhancer.')
    for name in variants:
        chosen=json.loads(a.model.read_text(encoding='utf-8'))
        chosen['name']=candidate['name']+' — '+name
        chosen['model']['label']=chosen['name'];chosen['model']['params']['4']=variants[name]
        (a.output/f'modelo-{name}.json').write_text(json.dumps(chosen,indent=2,ensure_ascii=False),encoding='utf-8')
    for stem,windows in passages.items():
        baseline=a.baseline/stem/'performance.f32'
        manifest=json.loads((a.baseline/stem/'render-manifest.json').read_text())
        assert manifest['binary_sha256']==sha(a.binary) and manifest['candidate_sha256']==sha(a.model)
        assert manifest['tuning']==tuning and manifest['params']==model['params']
        previous=np.memmap(baseline,dtype='<f4',mode='r').reshape(-1,2)
        reference=Path('C:/SHARED/SteinwayD-MIDI')/f'SteinwayD-StageORTF-{stem}.wav'
        metadata=json.loads(reference.with_suffix('.json').read_text(encoding='utf-8'))
        assert metadata['midi_sha256']==json.loads((Path('C:/SHARED/Graves-con-ataque/MIDI')/
            ('Fieltro-experimental-'+('La-Campanella' if stem.startswith('liszt') else 'Nocturno-op9-2')+'.json')).read_text())['midi_sha256']
        for start,end in windows:
            title=('Campanella' if stem.startswith('liszt') else 'Nocturno')+f'-{start}s-{end}s'
            clips={'Actual':previous[start*44100:end*44100].astype(float)*gain}
            for name in variants:
                x=np.memmap(paths[name,stem],dtype='<f4',mode='r').reshape(-1,2)
                clips[name]=x[start*44100:end*44100].astype(float)*gain
            clips['Steinway'],rate=sf.read(reference,start=start*44100,frames=(end-start)*44100,always_2d=True)
            assert rate==44100
            report['metrics'][title]={}
            for name,x in clips.items():
                assert np.isfinite(x).all() and abs(x).max()<1
                sf.write(a.output/f'{title}-{name}.wav',x,44100,subtype='PCM_24')
                sf.write(a.output/f'{title}-{name}-mono.wav',x.mean(axis=1),44100,subtype='PCM_24')
                row=stats(x);row['peak']=float(abs(x).max());report['metrics'][title][name]=row
                if title=='Campanella-36s-44s':print(name,row['None'],flush=True)
    (a.output/'validacion.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    (a.output/'LEEME.txt').write_text('APERTURA ESTÉREO SOBRE GRAVES CON ATAQUE\n'
        'Actual: ancho 0,35. Apertura-70: ancho 0,70. Apertura-100: ancho 1,00.\n'
        'Sólo cambia el control existente de ancho. Mismo contacto, graves, afinación y ganancia.\n'
        'Fragmentos nativos con todo el historial MIDI previo y pedales.\n'
        'Archivos -mono para comprobar el cambio al sumar los canales.\n'
        'Steinway: sampler de bitKlavier Grand Stage ORTF, referencia de captación.\n'
        'No se ha elegido ni instalado una nueva configuración.\n',encoding='utf-8')
    (a.output/'CREDITOS-Steinway.md').write_text(Path('local/reference-audio/bitklavier-grand/README.md').read_text(encoding='utf-8'),encoding='utf-8')


if __name__=='__main__':main()
