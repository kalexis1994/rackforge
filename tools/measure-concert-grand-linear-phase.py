"""Compare homogeneous coupled linear modes with their exact exponential.

Small synthetic C4/G4/C5 bank, not the living 606-coordinate piano state.
Keep the energy/work ledger and every dry carrier; no gain/phase alignment.
Store small transition matrices for independent reproduction, no large R.
"""
import argparse,json,sys,time
from pathlib import Path
import numpy as np
from scipy.linalg import eig,expm
from scipy.integrate import solve_ivp
from concert_grand_linear_phase import LinearBank,gauss4_maps
from concert_grand_wave_cache import digest


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True);p.add_argument('--data',type=Path,required=True)
    args=p.parse_args()
    if args.report.exists() or args.data.exists(): raise ValueError('preserve evidence')
    start=time.perf_counter();bank=LinearBank();L=bank.Lc
    sources={Path(m.__file__).name:digest(Path(m.__file__)) for m in list(sys.modules.values())
             if getattr(m,'__file__',None) and Path(m.__file__).resolve().parent==Path(__file__).resolve().parent}
    sources[Path(__file__).name]=digest(Path(__file__))
    report=dict(stage='construction',scope='Small synthetic linear C4/G4/C5, one wire per key, '
        '24 vertical/24 horizontal/48 axial modes per wire, three board modes, '
        'six Pade sections of acoustic rank two and six relaxation states. Heads parked. '
        'Continuous and discrete phase controls only; not the large piano or a sound fit.',
        all_gates_passed=False,approved_for_audition=False,source_sha256=sources,
        criteria=dict(energy_relative=1e-9,linear_map_relative=1e-9,ode_relative=1e-8,
                      endpoint_state_relative=.01,phase_frequency_relative=1e-4,
                      mechanical_energy_fraction=.5),cases=[],duration_s=.008,
        dimensions=dict(core=bank.n,complete_linear=bank.size,coupled=len(L),dry=len(bank.dry)))
    def save():
        report['seconds']=time.perf_counter()-start;args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save()
    values,vectors=eig(L)
    mechanical_count=len(bank.coupled)-bank.nr-bank.nz
    fractions=np.sum(abs(vectors[:mechanical_count])**2,axis=0)/np.sum(abs(vectors)**2,axis=0)
    freq=values.imag/(2*np.pi)
    chosen=np.flatnonzero((freq>=60)&(freq<=12000)&(fractions>=.5))
    chosen=chosen[np.argsort(freq[chosen])]
    modes=vectors[:,chosen]; modes/=np.linalg.norm(modes,axis=0)
    lam=values[chosen]; frequency=freq[chosen]
    if not len(chosen): raise ValueError('mechanical loaded modes required')
    rng=np.random.default_rng(20261009);initial=np.zeros(len(L))
    # Zero displacement/air and deterministic vertical+board velocity.
    half=mechanical_count//2;initial[half:mechanical_count]=rng.normal(size=half)*1e-7
    T=expm(report['duration_s']*L);expected=T@initial
    ode=solve_ivp(lambda t,x:L@x,(0,report['duration_s']),initial,
                  method='DOP853',rtol=1e-11,atol=1e-18,max_step=1/512000)
    relative=lambda a,b:float(np.linalg.norm(a-b)/np.linalg.norm(b))
    error=relative(ode.y[:,-1],expected)
    report['continuous_oracle']=dict(success=bool(ode.success),relative=error,
        passed=bool(ode.success and error<report['criteria']['ode_relative']))
    if not report['continuous_oracle']['passed']: raise ValueError('exact flow oracle failed')
    dissipation=-(L+L.T)/2
    checks=[]
    for _ in range(8):
        x=np.zeros(bank.size);x[bank.coupled]=rng.normal(size=len(L))*1e-7
        physical=sum(bank.powers(x).values());derivative=-float(x@bank.L@x)
        checks.append(abs(derivative-physical)/max(physical,1e-30))
    report['continuous_power_relative']=checks
    if max(checks)>report['criteria']['energy_relative']:raise ValueError('continuous physical energy identity failed')
    data=dict(generator=L,initial=initial,exact_final=expected,exact_flow=T,
              mode_values=lam,mode_vectors=modes,mode_frequency_hz=frequency,
              mode_mechanical_fraction=fractions[chosen])
    for rate in [32000,64000,128000,256000,512000]:
        dt=1/rate;steps=round(report['duration_s']*rate)
        exact=expm(dt*L); factor=np.exp(dt*lam)
        for label in ['split','port','gauss4']:
            report['stage']=f'{label}/{rate}';save();tick=time.perf_counter()
            M=bank.coupled_transition(label,dt)
            elapsed=time.perf_counter()-tick;data[f'{label}{rate}_transition']=M
            U=M@modes;mu=np.sum(modes.conj()*U,axis=0)
            phase=np.angle(mu/factor)/dt
            numerical_frequency=frequency+phase/(2*np.pi)
            frequency_relative=abs(numerical_frequency-frequency)/frequency
            decay_error=-np.log(abs(mu))/dt+lam.real
            leakage=np.linalg.norm(U-modes*mu,axis=0)/abs(factor)
            final=np.linalg.matrix_power(M,steps)@initial
            state_error=relative(final,expected);data[f'{label}{rate}_final']=final
            flow=np.linalg.matrix_power(M,steps)
            modes_final=flow@modes;target=modes*np.exp(report['duration_s']*lam)
            endpoint=np.linalg.norm(modes_final-target,axis=0)/np.linalg.norm(target,axis=0)
            full=np.zeros(bank.size);full[bank.coupled]=initial
            if label in bank.pilots:
                direct=bank.actual_step(label,full,dt)[bank.coupled]
                linear_error=relative(M@initial,direct)
            else: linear_error=0.
            max_gain=float(np.linalg.svd(M,compute_uv=False)[0])
            row=dict(label=label,rate_hz=rate,matrix_construction_seconds=elapsed,
                note='Matrix construction costs are not runtime speedups.',
                maximum_energy_norm_gain=max_gain,linear_map_relative=linear_error,
                state_endpoint_relative=state_error,maximum_mode_endpoint_relative=float(max(endpoint)),
                maximum_phase_frequency_relative=float(max(frequency_relative)),
                maximum_projection_leakage=float(max(leakage)),modes=[])
            for i,hz in enumerate(frequency):
                row['modes'].append(dict(frequency_hz=float(hz),decay_per_s=float(-lam[i].real),
                    mechanical_fraction=float(fractions[chosen[i]]),
                    projected_phase_frequency_hz=float(numerical_frequency[i]),
                    frequency_relative=float(frequency_relative[i]),decay_error_per_s=float(decay_error[i]),
                    projection_leakage_relative=float(leakage[i]),endpoint_relative=float(endpoint[i])))
            if label=='gauss4':
                _,Y=gauss4_maps(L,dt)
                closure=M.T@M-np.eye(len(L))+dt*sum(y.T@dissipation@y for y in Y)
                row['stage_work_matrix_relative']=float(np.linalg.norm(closure)/np.sqrt(len(L)))
                heat=0.;x=initial.copy()
                for _ in range(steps):
                    heat+=dt/2*sum(float((y@x)@dissipation@(y@x)) for y in Y)
                    x=M@x
                row.update(initial_energy_j=float(initial@initial/2),final_energy_j=float(x@x/2),
                    integrated_heat_j=heat,work_balance_relative=float(abs((x@x-initial@initial)/2+heat)/(initial@initial/2)))
            row['measurement_controls_passed']=bool(linear_error<report['criteria']['linear_map_relative']
                and max_gain<1+report['criteria']['energy_relative']
                and row.get('stage_work_matrix_relative',0)<report['criteria']['energy_relative']
                and row.get('work_balance_relative',0)<report['criteria']['energy_relative'])
            row['local_phase_and_state_passed']=bool(row['measurement_controls_passed']
                and state_error<report['criteria']['endpoint_state_relative']
                and max(endpoint)<report['criteria']['endpoint_state_relative']
                and max(frequency_relative)<report['criteria']['phase_frequency_relative'])
            report['cases'].append(row);save()
            print('PHASE',label,rate,'state',state_error,'mode end',max(endpoint),
                  'frequency',max(frequency_relative),'local',row['local_phase_and_state_passed'],flush=True)
    args.data.parent.mkdir(parents=True,exist_ok=True);np.savez_compressed(args.data,**data)
    report['data']=dict(path=str(args.data.resolve()),sha256=digest(args.data),bytes=args.data.stat().st_size)
    report['source_unchanged']=all(digest(Path(__file__).with_name(name))==sha for name,sha in sources.items())
    report['stage']='completed small coupled linear phase controls';save()


if __name__=='__main__':main()
