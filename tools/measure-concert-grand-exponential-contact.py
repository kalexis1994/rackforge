"""Contact carriers versus coupled midpoint, with living 606-board/rank-192 air.

Local tests only; preserve accepted WAVs and never overwrite reports/capsules.
Compare pressure AND energy-scaled mechanical/air states. Coarse initial
histories are shared, not asserted converged. No factory or audition selection.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from concert_grand_shared_strings import SharedHammers,SharedContact,SharedDampers
from concert_grand_shared_kernels import BatchedStrings,BatchedCoast
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_cell_audio import pressure_fft
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location('shared_kernel_audit',Path(__file__).with_name('measure-concert-grand-shared-kernels.py'))
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
frozen=audit.frozen


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    p.add_argument('--warm-state',type=Path,required=True)
    args=p.parse_args()
    if args.report.exists() or args.warm_state.exists():raise ValueError('preserve existing evidence')
    start=time.perf_counter()
    names=[Path(__file__).name,'concert_grand_exponential_contact.py',
        'measure-concert-grand-shared-kernels.py','concert_grand_shared_kernels.py',
        'concert_grand_shared_strings.py','render-concert-grand-shared-phrase.py',
        'concert_grand_audition_air.py']
    report=dict(stage='construction',all_gates_passed=False,approved_for_audition=False,
        scope='606 shared board, nine 96/192 wires, frozen rank-192 air. Local finite source-interval '
              'pressure/state comparison, not a full phrase or full acoustic accuracy certificate.',
        assumed_midi_velocity=84,source_sha256={n:digest(Path(__file__).with_name(n)) for n in names},cases=[])
    def save():
        report['seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();modal,geometry,air,kernel,paths=frozen.prepare(report,save)
    original,_=frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    contact=SharedContact(system,air);carrier=ExponentialContact(system,air)
    coast=BatchedCoast(system,air);dampers=SharedDampers(system.string,system.nh)
    initial=contact.initial_state(0.);initial,_=system.launch(initial,60,5*(84/124)**1.5)
    def metric(state):
        n=system.string.size;omega=np.sqrt(coast.oscillator_functions(1/128000)[3])
        return np.r_[omega*coast.project(state[0][:n]),state[1][:n]]
    def execute(label,pilot,state,duration,rate,damper=None):
        report['stage']=label+' '+str(rate);save()
        try:
            final,trace,stats=audit.run(pilot,state,duration,rate,damper)
        except RuntimeError as error:
            failure=dict(label=label,rate_hz=rate,failed=True,error=str(error))
            report['cases'].append(failure);save();print('REJECTED',failure,flush=True)
            return None
        pressure=pressure_fft(*trace,kernel)[0]
        if not np.isfinite(pressure).all():raise ValueError('nonfinite diagnostic pressure')
        stats.update(label=label,failed=False,pressure_peak_pa=float(abs(pressure).max()))
        report['cases'].append(stats);save();print('CASE',stats,flush=True)
        return final,pressure,stats
    def compare(item,reference):
        state,wave,stats=item;other,other_wave,ref=reference
        stats.update(reference_label=ref['label'],reference_rate_hz=ref['rate_hz'],
            pressure_relative=audit.relative(wave,other_wave),
            mechanical_state_relative=audit.relative(metric(state),metric(other)),
            wave_state_relative=audit.relative(state[2],other[2]),
            relaxation_state_relative=audit.relative(state[3],other[3]),
            speed_ratio=ref['seconds']/stats['seconds'])
        stats['local_pressure_1_percent_passed']=stats['pressure_relative']<=.01
        stats['local_mechanical_1_percent_passed']=stats['mechanical_state_relative']<=.01
        save()
    # Cold reference: previous audit measured its pressure/state against 768k.
    cold_ref=execute('cold_midpoint_reference',contact,initial,.002,1536000)
    if cold_ref is None:raise ValueError('reference failed')
    for rate in [64000,128000,256000]:
        item=execute('cold_exact_carrier',carrier,initial,.002,rate)
        if item is not None:compare(item,cold_ref)
    if system.compression(cold_ref[0][0])>=-1e-12:raise ValueError('extend measured reference contact interval')
    # A single warm capsule prevents loss of the paired initial condition.
    warmed,_,warm=audit.run(coast,cold_ref[0],.038,32000)
    args.warm_state.parent.mkdir(parents=True,exist_ok=True)
    np.savez(args.warm_state,q=warmed[0],v=warmed[1],wave=warmed[2],relaxation=warmed[3])
    report['warm_state']=dict(path=str(args.warm_state.resolve()),sha256=digest(args.warm_state),
        bytes=args.warm_state.stat().st_size,physical_time_s=.04,cold_reference_rate_hz=1536000,
        coast_rate_hz=32000,all_dampers_open=True,history_converged=False,metrics=warm,
        use='Paired local initial condition for this frozen board/air/strings; not an audition voice.')
    save()
    restrike,work=system.launch(audit.clone(warmed),60,5*(84/124)**1.5)
    report['restrike_external_work_j']=work
    refs=[]
    for rate in [1536000,3072000]:
        item=execute('restrike_midpoint_reference',contact,restrike,.002,rate)
        if item is None:raise ValueError('fine reference failed')
        refs.append(item)
    compare(refs[0],refs[1])
    for rate in [64000,128000,256000]:
        item=execute('restrike_exact_carrier',carrier,restrike,.002,rate)
        if item is not None:compare(item,refs[-1])
    releases=[]
    for rate in [32000,64000,128000]:
        item=execute('pedal_release',coast,warmed,.040,rate,dampers)
        if item is None:raise ValueError('release reference failed')
        releases.append(item)
    compare(releases[0],releases[1]);compare(releases[1],releases[2])
    report['source_and_board_unchanged']=all(digest(Path(__file__).with_name(n))==h for n,h in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256']
    report['stage']='completed local contact carriers and release audit';save()
    print('AUDIT COMPLETED',args.report,flush=True)


if __name__=='__main__':main()
