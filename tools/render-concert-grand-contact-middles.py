"""Focal C4/F4/G4/C5 auditions, with one frozen SFZ gain and three velocities."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import numpy as np
import soundfile as sf


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--candidate',type=Path,required=True)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--work',type=Path,required=True)
    p.add_argument('--baseline',type=Path,required=True)
    p.add_argument('--evaluation',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.work.mkdir(exist_ok=True,parents=True)
    e=module(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'),'e')
    sampler=module(Path(__file__).with_name('render-bitklavier-steinway-midi.py'),'sampler')
    chosen=json.loads(a.candidate.read_text());frozen={**chosen['model'],'tuning':{**chosen['model']['tuning'],**chosen['overrides']}}
    assert e.digest(a.binary)==frozen['binary_sha256']
    evaluation=json.loads(a.evaluation.read_text());gain=10**(evaluation['gain_db']/20)
    synth=sampler.Engine(Path('local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'),Path('local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz'))
    pairs=[(n,v) for n in (60,65,67,72) for v in (60,84,124)]
    clips={label:[] for label in ('Fieltro-experimental','14B-anterior','Steinway-sampler')};measurements={}
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures={pair:pool.submit(e.render_model,a.binary,frozen,a.work/f'{pair[0]}-{pair[1]}',*pair) for pair in pairs}
        for n,v in pairs:
            ref=e.render_reference(synth,n,v);candidate=e.read_model(futures[n,v].result())*gain
            baseline=e.read_model(a.baseline/f'{n}-{v}'/'performance.f32')*gain
            row=evaluation['rows'][f'{n}:{v}']
            original,rate=sf.read(row['source_file'],frames=e.RATE,always_2d=True)
            support=min(11.5,(row['source_seconds']-e.onset(original)/rate)/2**(row['transposition_semitones']/12))
            measurements[f'{n}:{v}']=dict(transposition=row['transposition_semitones'],
                baseline=e.multi_spectrum(ref,baseline,support),
                candidate=e.multi_spectrum(ref,candidate,support))
            for label,x in (('Fieltro-experimental',candidate),('14B-anterior',baseline),('Steinway-sampler',ref)):
                clips[label].append(x[:3*e.RATE])
    synth.close();peak=max(abs(x).max() for values in clips.values() for x in values);trim=min(1,.85/peak)
    for label,values in clips.items():
        pieces=[]
        for x in values:
            x=x.copy()*trim;x[-662:]*=np.linspace(1,0,662)[:,None]
            pieces.extend((x,np.zeros((round(.4*e.RATE),2))))
        x=np.concatenate(pieces);assert np.isfinite(x).all() and abs(x).max()<=.851
        sf.write(a.output/(label+'-medios.wav'),x,e.RATE,subtype='PCM_24')
    (a.output/'medios.json').write_text(json.dumps(dict(pairs=pairs,measurements=measurements,gain_db=evaluation['gain_db'],
        common_trim_db=float(20*np.log10(trim)),held_seconds=3,silence_seconds=.4,
        source='Frozen SFZ; F4 uses F#4 minus1 semitone, G4 uses F#4 plus1. C4/C5 exact. Velocity-bin and release rules are player rules.'),indent=2))
    print('DONE focal audition',flush=True)


if __name__=='__main__':main()
