"""Harmonic midpoint frequency warp of the frozen physical acoustic pilot.

Use the same reduced wave and relaxation, no contact trajectory or new R.
Z evaluated at i*w versus i*2/h*tan(w*h/2). Displacement-port load also
uses 2/h*sin(w*h/2). These are operator diagnostics, not microphone errors.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location('frozen_coast_controls',Path(__file__).with_name('measure-concert-grand-coast-controls.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-report',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);p.add_argument('--data',type=Path,required=True)
    args=p.parse_args()
    if args.report.exists() or args.data.exists():raise ValueError('preserve measured evidence')
    prior=json.loads(args.base_report.read_text());names=prior['source_sha256'].copy()
    if not prior['stage'].startswith('completed') or not prior['source_and_cache_unchanged']:
        raise ValueError('completed unchanged base required')
    for name,sha in names.items():
        if digest(Path(__file__).with_name(name))!=sha:raise ValueError('changed source '+name)
    names[Path(__file__).name]=digest(Path(__file__))
    start=time.perf_counter();report=dict(stage='construction',all_gates_passed=False,
        approved_for_audition=False,source_sha256=names,base_report_sha256=digest(args.base_report),
        scope='Actual rank192 reduced acoustic operator of the 606-coordinate prototype. '
              'Harmonic impedance and displacement-load matrices, not full trajectory, '
              'loaded resonance or microphone pressure. No Gauss replacement selected.',cases=[])
    def save():
        report['seconds']=time.perf_counter()-start;args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();_,_,air,_,paths=base.base.base.audit.frozen.prepare(report,save)
    frequency=np.array([261.625565,391.995436,523.251131,1000.,2500.,6000.,10000.,12000.])
    data={'frequency_hz':frequency}
    relative=lambda a,b:float(np.linalg.norm(a-b)/np.linalg.norm(b))
    def Z(s):
        # Bound the preserved parallel relaxation explicitly; its generic
        # base class assumes as many states as port coordinates.
        wave=air.wave.impedance(s)
        relaxation=(air.relaxation.B*(s*air.relaxation.tau/(1+s*air.relaxation.tau)))@air.relaxation.B.T
        return wave+relaxation
    for i,hz in enumerate(frequency):
        omega=2*np.pi*hz;physical=Z(1j*omega);data[f'physical{i}']=physical
        for rate in [32000,64000,128000,256000]:
            dt=1/rate;psi=omega*dt/2
            warped=2/dt*np.tan(psi);displacement=2/dt*np.sin(psi)
            discrete=Z(1j*warped);data[f'discrete{i}_{rate}']=discrete
            load=1j*displacement*discrete;continuous=1j*omega*physical
            row=dict(frequency_hz=float(hz),rate_hz=rate,midpoint_frequency_hz=float(warped/(2*np.pi)),
                frequency_warp_relative=float(warped/omega-1),
                impedance_relative=relative(discrete,physical),
                displacement_load_relative=relative(load,continuous),
                physical_impedance_norm=float(np.linalg.norm(physical)),
                criterion='No acoustic/microphone convergence gate inferred from this matrix norm.')
            report['cases'].append(row);save()
        print('AIR WARP',hz,[r['displacement_load_relative'] for r in report['cases'][-4:]],flush=True)
    args.data.parent.mkdir(parents=True,exist_ok=True);np.savez_compressed(args.data,**data)
    report['data']=dict(path=str(args.data.resolve()),sha256=digest(args.data),bytes=args.data.stat().st_size)
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==s for k,s in names.items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['stage']='completed actual acoustic harmonic warp controls';save()


if __name__=='__main__':main()
