"""Actual 606-board/9-wire algebraic speed and local time-refinement audit.

Finite source-interval pressure, not a new listening WAV or complete phrase
convergence certificate. All tested rates start from the same living state.
Original producer, reference WAVs and the single full R cache stay intact.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_shared_strings import SharedHammers,SharedContact,SharedCoast,SharedDampers
from concert_grand_shared_kernels import BatchedStrings,BatchedCoast
from concert_grand_damped_coast import engagement
from concert_grand_cell_audio import pressure_fft
from concert_grand_wave_cache import digest

spec=importlib.util.spec_from_file_location('frozen_shared_phrase',Path(__file__).with_name('render-concert-grand-shared-phrase.py'))
frozen=importlib.util.module_from_spec(spec);spec.loader.exec_module(frozen)


def clone(state):return tuple(x.copy() for x in state)


def run(pilot,state,duration,rate,damper=None):
    core=pilot.system.string;state=clone(state);count=round(duration*rate)
    if count/rate!=duration:raise ValueError('aligned audit duration required')
    def board(x):return core.board_transform@x[core.sb]
    times=[0.];B=[board(state[0])];V=[board(state[1])]
    felt=material=heat=0.;force=0.;start=last=time.perf_counter()
    capture=max(1,rate//128000)
    for i in range(count):
        dt=1/rate
        if damper is not None:
            levels={k:engagement((i+.5)/rate) for k in pilot.system.keys}
            v,h=damper.dissipate(state[1],dt/2,levels);heat+=h
            state=(state[0],v,state[2],state[3])
        state,stats=pilot.step(state,dt,True)
        if damper is not None:
            v,h=damper.dissipate(state[1],dt/2,levels);heat+=h
            state=(state[0],v,state[2],state[3])
        felt+=stats['felt_loss_j'];material+=stats['material_loss_j']
        force=max(force,stats['force_n'])
        if (i+1)%capture==0 or i==count-1:
            times.append((i+1)/rate);B.append(board(state[0]));V.append(board(state[1]))
        if time.perf_counter()-last>25:
            print('audit progress',rate,(i+1)/rate,'/',duration,flush=True);last=time.perf_counter()
    elapsed=time.perf_counter()-start
    uniform=np.arange(round(duration*128000)+1)/128000
    spline=CubicHermiteSpline(times,np.asarray(B),np.asarray(V),axis=0)
    return state,(uniform,spline(uniform),spline(uniform,1)),dict(seconds=elapsed,
        rate_hz=rate,source_duration_s=duration,felt_heat_j=felt,board_material_heat_j=material,
        damper_heat_j=heat,max_felt_force_n=force,final_compression_m=pilot.system.compression(state[0]))


def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True)
    args=p.parse_args()
    if args.report.exists():raise ValueError('preserve completed reports')
    start=time.perf_counter()
    report=dict(stage='construction',all_gates_passed=False,full_phrase_convergence=False,
        scope='Same 606-board, nine wires, 96/192 modes, frozen rank-192 air. Local source-interval '
              'retarded pressure and all living states. Not a full-keyboard, full-air or waveform certificate.',
        pressure_metric='Pressure contributed by the tested finite source interval, including its delayed tail; '
                        'not the complete microphone history before that interval.',
        source_sha256={n:digest(Path(__file__).with_name(n)) for n in [Path(__file__).name,
            'concert_grand_shared_kernels.py','concert_grand_shared_strings.py',
            'render-concert-grand-shared-phrase.py','concert_grand_audition_air.py']},cases=[])
    def save():
        report['total_seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();modal,geometry,air,kernel,paths=frozen.prepare(report,save)
    original,_=frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    old_contact=SharedContact(original,air);contact=SharedContact(system,air)
    old_coast=SharedCoast(original,air);coast=BatchedCoast(system,air)
    damper=SharedDampers(system.string,system.nh)
    state=contact.initial_state(0.);state,_=system.launch(state,60,5*(84/124)**1.5)
    def pressure(trace):return pressure_fft(*trace,kernel)[0]
    def pair(label,pilots,initial,duration,rate):
        a,ta,ma=run(pilots[0],initial,duration,rate)
        b,tb,mb=run(pilots[1],initial,duration,rate)
        metrics=dict(label=label,reference=ma,batched=mb,speed_ratio=ma['seconds']/mb['seconds'],
            pressure_relative=relative(pressure(tb),pressure(ta)),
            state_relative=[relative(x,y) for x,y in zip(b,a)])
        metrics['equivalence_passed']=metrics['pressure_relative']<1e-8 and max(metrics['state_relative'])<1e-8
        report['cases'].append(metrics);save();print('PAIR',metrics,flush=True)
        return b
    pair('contact_equivalence',(old_contact,contact),state,.0005,768000)
    # Cold contact and a later living restrike both refine the nonlinear contact.
    def refine(label,pilot,initial,duration,rates,dampers=None):
        results=[]
        for rate in rates:
            final,trace,metrics=run(pilot,initial,duration,rate,dampers)
            metrics.update(label=label,pressure_peak_pa=float(abs(pressure(trace)).max()))
            results.append((final,pressure(trace),metrics))
            report['cases'].append(metrics);save();print('REFINE',label,metrics,flush=True)
        for index,(final,wave,metrics) in enumerate(results[:-1]):
            nxt,pressure_next,_=results[index+1]
            metrics['next_rate_hz']=rates[index+1]
            metrics['pressure_relative_to_next']=relative(wave,pressure_next)
            n=system.string.size
            # Energy-scaled mechanical displacement/velocity; head flight excluded.
            omega=np.sqrt(coast.oscillator_functions(1/128000)[3])
            a=np.r_[omega*coast.project(final[0][:n]),final[1][:n]]
            b=np.r_[omega*coast.project(nxt[0][:n]),nxt[1][:n]]
            metrics['mechanical_state_relative_to_next']=relative(a,b)
            metrics['wave_state_relative_to_next']=relative(final[2],nxt[2])
            metrics['relaxation_state_relative_to_next']=relative(final[3],nxt[3])
            metrics['local_pressure_1_percent_passed']=metrics['pressure_relative_to_next']<=.01
        save()
        return results[-1][0]
    separated=refine('cold_contact',contact,state,.002,[768000,1536000])
    if system.compression(separated[0])>=-1e-12:raise ValueError('contact still active; lengthen measured contact interval')
    pair('coast_equivalence',(old_coast,coast),separated,.002,16000)
    refine('loaded_coast',coast,separated,.010,[16000,32000,64000])
    warmed,_,warm_metrics=run(coast,separated,.038,16000)
    report['warm_state']=dict(physical_time_s=.04,prior_contact_rate_hz=1536000,
        prior_coast_rate_hz=16000,all_dampers_open=True,metrics=warm_metrics,
        scope='Identical local initial condition for refinement; preceding trajectory is not converged.')
    save()
    restrike,work=system.launch(clone(warmed),60,5*(84/124)**1.5)
    report['restrike_external_work_j']=work
    refine('living_restrike',contact,restrike,.002,[768000,1536000])
    refine('pedal_release',coast,warmed,.040,[16000,32000,64000],damper)
    report['algebraic_equivalence_passed']=all(c['equivalence_passed'] for c in report['cases'] if 'equivalence_passed' in c)
    report['local_pressure_refinement_passed']=all(c['local_pressure_1_percent_passed'] for c in report['cases'] if 'local_pressure_1_percent_passed' in c)
    report['source_and_board_unchanged']=all(digest(Path(__file__).with_name(n))==h for n,h in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256']
    report['stage']='completed local speed and refinement audit';save()
    print('AUDIT COMPLETED',args.report,flush=True)


if __name__=='__main__':main()
