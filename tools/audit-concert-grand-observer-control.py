"""Compare known causal native observations with learned reconstruction.

The impulse responses are probed from the frozen model, not fitted to Steinway
or to these notes. They provide an oracle upper control for identification.
Every new source capture must reproduce both old audio and old 16-point force
bytes exactly. The extra original-force input was absent from the first refit.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('binary','oracle','original','work','output'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--capture-binary',type=Path)
    a=parser.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    tools=Path(__file__).resolve().parent
    body=module(tools/'identify-concert-grand-body.py','observer_body')
    control=module(tools/'reconstruct-concert-grand-current.py','observer_control')
    meta=json.loads((a.oracle/'oracle.json').read_text())
    assert meta['sources']==17 and meta['serial_span']==128 and meta['rate']==body.RATE
    kernel_path=a.oracle/'board-room-kernels.f32'
    kernels=np.array(np.fromfile(kernel_path,dtype='<f4').reshape(meta['kernel_count'],meta['frames'],2).transpose(0,2,1),dtype=float)
    def held_kernels(note):
        assert meta['muted_owner_first'] <= note < meta['muted_owner_first']+meta['muted_owner_count']
        return np.concatenate((kernels[:16],kernels[17+note-meta['muted_owner_first']:18+note-meta['muted_owner_first']]))
    report=dict(policy=__doc__,probe=meta,kernel_sha256=body.FIT.AUDIT.sha(kernel_path),
                probe_binary_sha256=body.FIT.AUDIT.sha(a.binary),groups={},sources={},export_gain_db=0,
                capture_binary_sha256=body.FIT.AUDIT.sha(a.capture_binary or a.binary),
                caveats=['Known model kernels are not identified real-piano responses.',
                         'Known native owner mask is used for each held key; no per-note fitted coefficients.',
                         'An arbitrary performance needs time-varying native muting, not one fixed held-key kernel.',
                         'FIR duration is 3 seconds. Output nonlinearities are not fitted.'])
    def capture(pair,seconds=1.7):
        n,v=pair;folder='audition-capture' if seconds>2 else 'capture'
        directory=a.work/folder/f'{n}-{v}'
        x,y=body.capture(a.capture_binary or a.binary,directory,n,v,seconds)
        old=a.original/folder/f'{n}-{v}'
        hashes={}
        for filename in ('bridge-drive.f32','performance.f32'):
            digest=body.FIT.AUDIT.sha(directory/filename)
            assert digest==body.FIT.AUDIT.sha(old/filename),f'Baseline changed: {n}:{v}/{filename}'
            hashes[filename]=digest
        auxiliary=directory/'auxiliary-drive.f32'
        aux=np.fromfile(auxiliary,dtype='<f4').astype(float)
        assert len(aux)==len(x) and np.isfinite(aux).all()
        hashes['auxiliary-drive.f32']=body.FIT.AUDIT.sha(auxiliary)
        return pair,np.column_stack((x,aux)),y,hashes
    for label,pairs in [('training',body.TRAIN),('reserved-pitches',body.PITCH_TEST),('reserved-velocities',body.VELOCITY_TEST)]:
        rows,board_rows={},{ }
        with ThreadPoolExecutor(max_workers=2) as pool:
            for (n,v),x,y,hashes in pool.map(capture,pairs):
                key=f'{n}:{v}';report['sources'][key]=hashes
                board=control.apply(x[:,:16],kernels[:16]);all_routes=control.apply(x,held_kernels(n))
                rows[key]=control.metrics(y,all_routes,n);board_rows[key]=control.metrics(y,board,n)
        report['groups'][label]=dict(board_only=control.aggregate(board_rows),with_auxiliary=control.aggregate(rows),
                                     board_only_rows=board_rows,with_auxiliary_rows=rows)
        print(label,'board',json.dumps(report['groups'][label]['board_only']),
              'with auxiliary',json.dumps(report['groups'][label]['with_auxiliary']),flush=True)
        (a.output/'oracle-report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    parts={k:[] for k in ('Actual','Cadena-conocida')};audition_rows={}
    for n in (42,48,60,72,84,90):
        for v in (28,60,92,124):
            _,x,y,hashes=capture((n,v),4.)
            prediction=control.apply(x,held_kernels(n))
            report['sources'][f'{n}:{v}:audition']=hashes
            audition_rows[f'{n}:{v}']=control.metrics(y,prediction,n)
            for label,z in (('Actual',y),('Cadena-conocida',prediction)):
                sf.write(a.output/f'{n}-{v}-{label}.wav',z,body.RATE,subtype='FLOAT')
                clip=z[:round(2.5*body.RATE)].copy();clip[-441:]*=np.linspace(1,0,441)[:,None]
                parts[label].extend((clip,np.zeros((body.RATE//2,2))))
    for label,clips in parts.items():sf.write(a.output/f'{label}-intensidades.wav',np.concatenate(clips),body.RATE,subtype='FLOAT')
    report['audition']=dict(summary=control.aggregate(audition_rows),rows=audition_rows)
    report['passed']=all(g['with_auxiliary']['passed'] for g in report['groups'].values()) and report['audition']['summary']['passed']
    (a.output/'oracle-report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('DONE',report['passed'],a.output,flush=True)


if __name__=='__main__':main()
