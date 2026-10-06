"""Full-board retarded microphone kernel against signed dense cells and Green pressure.

Operator preparation only; no source trajectory, WAV or second acoustic R.
All 606 mass-normalized shapes and all quadrature points are retained.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path

import numpy as np

from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel, kernel_transfer
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location(
    'full_mic_reference',Path(__file__).with_name('audit-concert-grand-pade-contact-full.py'))
full=importlib.util.module_from_spec(spec)
spec.loader.exec_module(full)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','board','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--direction-block',type=int,default=32)
    p.add_argument('--max-array-gib',type=float,default=8.)
    a=p.parse_args()
    if a.report.exists():raise ValueError('new report required')
    if not np.isfinite(a.max_array_gib) or a.max_array_gib<=0:raise ValueError('positive array budget required')
    start=time.perf_counter()
    names=[Path(__file__).name,'audit-concert-grand-pade-contact-full.py',
           'concert_grand_cell_observer.py','concert_grand_cell_audio.py']
    r=dict(stage='surface preparation',all_gates_passed=False,preflight_only=True,
        criteria=dict(aggregation_relative=1e-12,green_transfer_relative=.0002),
        parent_sha256={str(a.full_report):digest(a.full_report)},board_sha256=digest(a.board),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in names},
        scope='Full assumed board pressure operator only. No contact/coast trajectory or audible-note approval.')

    def checkpoint():
        r['seconds']=time.perf_counter()-start
        r['peak_working_set_bytes']=full.reference.peak_memory()
        a.report.parent.mkdir(parents=True,exist_ok=True)
        a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')

    checkpoint()
    approved,_,_,_,_,provider,_,shapes=full.prepare_reference(a)
    r['physical_source_sha256']=approved['source_sha256']
    microphones=[[.55,.5,.66],[.85,.5,.66]]
    observer=CellObserver(provider.surface,microphones,4)
    rate=128000
    taps=int(np.floor(observer.distance.max()/observer.surface.speed*rate))+3
    cells=shapes.shape[0]
    points=observer.distance.shape[0]*cells
    # Conservative COO+CSR scratch count; allocator/OS peak measured separately.
    sparse_scratch=points*(8+8+8+8+4)+4*(taps+1)
    dense_scratch=taps*cells*8
    kernel_bytes=taps*len(microphones)*provider.modes*8
    estimate=provider.budget(real_gram=True)['estimated_array_peak_bytes']+dense_scratch+4*kernel_bytes+sparse_scratch
    if estimate>a.max_array_gib*2**30:raise MemoryError('declared complete array budget exceeded')
    r.update(modes=provider.modes,cells=approved['cells'],rate=rate,microphones=microphones,
        quadrature_order=4,taps=taps,array_estimate_bytes=estimate,
        scratch_bytes=dict(dense_histogram=dense_scratch,sparse_upper_estimate=sparse_scratch),
        kernel_bytes=kernel_bytes)
    r['stage']='kernel aggregation'
    checkpoint()
    kernels={}
    timings={}
    for method in ['reference','sparse']:
        t=time.perf_counter();kernels[method]=retarded_kernel(observer,rate,method)
        timings[method]=time.perf_counter()-t
        print('kernel',method,timings[method],flush=True)
    reference=kernels['reference'];candidate=kernels['sparse']
    r['kernel_seconds']=timings
    r['aggregation_relative']=float(np.linalg.norm(candidate-reference)/np.linalg.norm(reference))
    r['aggregation_maximum_absolute']=float(np.max(abs(candidate-reference)))
    frequency=np.array([0.,60.,261.,800.,2000.,4500.,6000.])
    actual=kernel_transfer(candidate,rate,frequency)
    exact=observer.acceleration_transfer(frequency)
    r['continuous_green_controls']=[dict(frequency_hz=float(hz),relative=float(np.linalg.norm(a-b)/np.linalg.norm(b)))
        for hz,a,b in zip(frequency,actual,exact)]
    first=int(np.floor(observer.distance.min()/observer.surface.speed*rate))-1
    r['kernel_gates']=dict(signed_aggregation=r['aggregation_relative']<r['criteria']['aggregation_relative'],
        continuous_green=max(row['relative'] for row in r['continuous_green_controls'])<r['criteria']['green_transfer_relative'],
        causal_first_arrival=bool(np.all(candidate[:first]==0)),
        source_unchanged=all(digest(Path(__file__).with_name(name))==sha for name,sha in r['source_sha256'].items()))
    r['kernel_gates_passed']=all(r['kernel_gates'].values())
    r['stage']='full microphone kernel audit completed'
    r['next']='Time pressure and capture refinement with the full living contact/coast source; no rate or timbral approval here.'
    checkpoint()
    print('finished',json.dumps(r['kernel_gates']),r['seconds'],flush=True)


if __name__=='__main__':main()
