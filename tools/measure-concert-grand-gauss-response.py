"""Gauss harmonic constant-port response of the frozen real air operator.

Reuse the previous physical impedance matrices from their sole local cache.
Save only row energies of errors, not another archive of full matrices/R.
This isolates operator frequency response; it is not a temporal certificate.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_gauss_response import harmonic_impedance

spec=importlib.util.spec_from_file_location('frozen_coast_controls',Path(__file__).with_name('measure-concert-grand-coast-controls.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--warp-report',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);p.add_argument('--data',type=Path,required=True)
    args=p.parse_args()
    if args.report.exists() or args.data.exists():raise ValueError('preserve evidence')
    old=json.loads(args.warp_report.read_text());names=old['source_sha256'].copy()
    if not old['stage'].startswith('completed') or not old['source_and_cache_unchanged']:
        raise ValueError('completed unchanged operator evidence required')
    for n,sha in names.items():
        if digest(Path(__file__).with_name(n))!=sha:raise ValueError('changed source '+n)
    archive=Path(old['data']['path'])
    if digest(archive)!=old['data']['sha256']:raise ValueError('changed physical impedance archive')
    names.update({n:digest(Path(__file__).with_name(n)) for n in [Path(__file__).name,'concert_grand_gauss_response.py']})
    start=time.perf_counter();report=dict(stage='construction',all_gates_passed=False,
        approved_for_audition=False,source_sha256=names,warp_report_sha256=digest(args.warp_report),
        original_operator_data_sha256=digest(archive),cases=[],
        scope='Actual rank192/606 port air; harmonic Gauss response under constant interval velocity. '
              'No matrix/operator copy to disk, contact, living coast or microphone error.')
    def save():
        report['seconds']=time.perf_counter()-start;args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();_,_,air,_,paths=base.base.base.audit.frozen.prepare(report,save)
    def Z(s):
        B=air.relaxation.B;t=air.relaxation.tau
        return air.wave.impedance(s)+(B*(s*t/(1+s*t)))@B.T
    saved={}
    with np.load(archive,allow_pickle=False) as archive_data:
        frequency=archive_data['frequency_hz']
        for hz in [6000.,10000.,12000.]:
            i=int(np.flatnonzero(frequency==hz)[0]);physical=archive_data[f'physical{i}']
            w=2*np.pi*hz;energy=np.sum(abs(w*physical)**2,axis=1)
            key=f'physical{i}_load_row_energy';saved[key]=energy
            for rate in [32000,64000,128000,256000]:
                dt=1/rate;discrete=harmonic_impedance(Z,w,dt);omega=2/dt*np.sin(w*dt/2)
                delta=np.sum(abs(omega*discrete-w*physical)**2,axis=1)
                dkey=f'gauss{i}_{rate}_delta_row_energy';saved[dkey]=delta
                error=float(np.sqrt(delta.sum()/energy.sum()))
                previous=next(c for c in old['cases'] if c['frequency_hz']==hz and c['rate_hz']==rate)
                report['cases'].append(dict(frequency_hz=hz,rate_hz=rate,load_relative=error,
                    midpoint_load_relative=previous['displacement_load_relative'],
                    reference_row_energy_key=key,delta_row_energy_key=dkey,
                    reciprocity_relative=float(np.linalg.norm(discrete-discrete.T)/np.linalg.norm(discrete))))
                save()
            print('GAUSS RESPONSE',hz,[x['load_relative'] for x in report['cases'][-4:]],flush=True)
    args.data.parent.mkdir(parents=True,exist_ok=True);np.savez_compressed(args.data,**saved)
    report['data']=dict(path=str(args.data.resolve()),sha256=digest(args.data),bytes=args.data.stat().st_size)
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==s for k,s in names.items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['stage']='completed actual Gauss air harmonic constant-port controls';save()


if __name__=='__main__':main()
