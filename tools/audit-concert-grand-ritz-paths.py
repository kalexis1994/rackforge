"""Separate bridge conductance from causal microphone pressure at fixed force.

Frozen native hybrid poles/masses and the actual sixteen-point source mapping.
No strings or note envelopes in this diagnostic. Mechanical conductance is
not itself a predicted decay time; the renderer also uses string normalization,
collective projections, intrinsic losses and nonlinear motion.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np


def module(name, filename):
    spec=importlib.util.spec_from_file_location(name,Path(__file__).with_name(filename))
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--expected-sha256',required=True)
    p.add_argument('--structural',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    assert sha(a.binary)==a.expected_sha256
    old=module('old_observer','audit-concert-grand-radiation-transfer.py')
    resolution=module('resolution','audit-concert-grand-ritz-resolution.py')
    structural=json.loads((a.structural/'report.json').read_text())
    profiles={'Base':None,**{k:a.structural/f'{k}-native.csv' for k in structural['banks']}}
    report=dict(policy=__doc__,binary_sha256=sha(a.binary),script_sha256=sha(Path(__file__)),
                structural_report_sha256=sha(a.structural/'report.json'),profiles={})
    cases=[(n,h,440*2**((n-69)/12)*h) for n in (60,72,84,90) for h in (1,2,3)]
    hz=np.array([c[2] for c in cases])
    tuning=a.output/'tuning.txt';tuning.write_text('',encoding='utf-8')
    for name,asset in profiles.items():
        export=a.output/f'{name}-transfer.csv'
        env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_TUNING=str(tuning.resolve()),CG_PHYSICAL_TRANSFER_CSV=str(export.resolve()),RUST_MIN_STACK='33554432')
        if asset:env['CG_RITZ_BOARD']=str(asset.resolve())
        with (a.output/f'{name}-export.log').open('w') as log:
            subprocess.run([str(a.binary.resolve()),'bench::export_physical_transfer','--exact','--ignored','--nocapture','--test-threads=1'],
                           env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        bank=np.genfromtxt(export,delimiter=',',names=True)
        native=None if asset is None else np.loadtxt(asset,delimiter=',')
        count=0 if native is None else len(native)
        # Upper complement stays on the exact native factory quadrature.
        _,distances,weights=old.surface(bank[count:],1)
        high_observers=np.stack([old.rayleigh(f,distances,weights,float(bank['common_path'][0]),True) for f in hz],axis=1)
        if count:
            x,y=np.meshgrid((np.arange(40)+.5)/40*1.4,(np.arange(24)+.5)/24-.5,indexing='ij')
            low_observers=resolution.observer(native[:,33:],x.ravel(),y.ravel(),bank,hz,True)
            observers=np.concatenate((low_observers,high_observers))
        else:observers=high_observers
        row=[]
        for fi,(note,harmonic,f) in enumerate(cases):
            coupling=old.projection(bank,note)
            if count:
                coupling[:count]=resolution.interpolate(native[:,1:17].T,np.array([(note-21)/87]))[0]
            velocity=old.mobility(bank,f)
            conductance=(velocity.real*coupling**2)
            assert conductance.min()>-1e-10
            pressure=(velocity[:,None]*coupling[:,None]*observers[:,fi]).sum(axis=0)
            level=float(10*np.log10(max(np.mean(abs(pressure)**2),1e-40)))
            row.append(dict(note=note,harmonic=harmonic,frequency_hz=f,
                            bridge_conductance=float(conductance.sum()),
                            low_bridge_conductance=float(conductance[:count].sum()),
                            pressure_pair_db=level))
        report['profiles'][name]=dict(export_sha256=sha(export),rows=row)
    base=report['profiles']['Base']['rows']
    for name,profile in report['profiles'].items():
        for row,control in zip(profile['rows'],base):
            row['conductance_ratio_to_base']=row['bridge_conductance']/control['bridge_conductance']
            row['pressure_minus_base_db']=row['pressure_pair_db']-control['pressure_pair_db']
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    for name,p in report['profiles'].items():
        print(name,[(r['note'],round(r['conductance_ratio_to_base'],3),round(r['pressure_minus_base_db'],3))
                    for r in p['rows'] if r['harmonic']==1],flush=True)


if __name__=='__main__':main()
