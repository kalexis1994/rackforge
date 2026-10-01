"""Compare baked factory defaults with the frozen native audition renderer."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('reference-binary','production-binary','profile','work'):
        p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args()
    profile=json.loads(a.profile.read_text(encoding='utf-8'))
    rate=44100
    events=[(0,176,64,127)]
    for i,(note,velocity) in enumerate([(42,84),(60,60),(72,124),(96,124),
            (70,45),(71,50),(70,54),(71,57),(70,39),(71,58)]):
        at=round((.1+i*.24)*rate)
        events.extend([(at,144,note,velocity),(at+round(.12*rate),128,note,64)])
    events.append((round(2.7*rate),176,64,0))
    events.sort()
    paths={}
    for label,binary in [('reference',a.reference_binary),('production',a.production_binary)]:
        folder=a.work/label
        folder.mkdir(parents=True,exist_ok=True)
        (folder/'events.txt').write_text('\n'.join(' '.join(map(str,event)) for event in events)+'\n',encoding='utf-8')
        env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str((folder/'events.txt').resolve()), CG_TOTAL_FRAMES=str(4*rate),
                   CG_RENDER_DIR=str(folder.resolve()),CG_RATE=str(rate),CG_PRESET='concert',
                   RUST_MIN_STACK='33554432')
        if label=='reference':
            tuning=folder/'tuning.txt'
            tuning.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(profile['tuning'].items()))+'\n',encoding='utf-8')
            env.update(CG_TUNING=str(tuning.resolve()),CG_PARAMS=','.join(f'{k}={v}' for k,v in profile['params'].items()))
        with (folder/'render.log').open('w',encoding='utf-8') as log:
            subprocess.run([str(binary.resolve()),'--exact','tests::render_reference_wavs',
                            '--ignored','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        paths[label]=folder/'performance.f32'
    original=np.fromfile(paths['reference'],dtype='<f4').reshape(-1,2)
    actual=np.fromfile(paths['production'],dtype='<f4').reshape(-1,2)
    expected=original*np.float32(profile['output_gain'])
    # Match the existing output ceiling, linear below 0.9.
    magnitude=abs(expected)
    over=(magnitude-.9)/.1
    mask=magnitude>.9
    expected[mask]=np.sign(expected[mask])*(.9+.1*over[mask]/(1+over[mask]))
    assert actual.shape==expected.shape and np.isfinite(actual).all()
    error=float(abs(actual-expected).max())
    assert error<2e-6, f'Baked defaults differ from the audition: {error}'
    report=dict(version=profile['version'],max_absolute_error=error,
                reference_binary_sha256=sha(a.reference_binary),production_binary_sha256=sha(a.production_binary),
                profile_sha256=sha(a.profile),frames=len(actual),rate=rate,
                peak=float(abs(actual).max()),gain_db=profile['output_gain_db'],
                scope='Factory default, same MIDI with pedal and restrikes at 256 frames. Existing ceiling only above 0.9.')
    (a.work/'validacion.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report,indent=2),flush=True)


if __name__=='__main__':
    main()
