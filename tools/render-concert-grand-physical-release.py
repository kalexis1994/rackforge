"""Continue existing live note states through passive note-off; save only tail.

The original cold attack/sustain assets are reused, never duplicated or edited.
Paired 64/128k release checks have the same damper placement and lowering.
"""
import argparse,json,time
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from concert_grand_air_coefficients import load_prefix
from concert_grand_bounded_air import BoundedAir
from concert_grand_damped_coast import DampedCoast,engagement
from concert_grand_cell_audio import pressure_fft
from concert_grand_cell_observer import spectral_relative
from scipy.interpolate import CubicHermiteSpline

def module(name,path):
    s=spec_from_file_location(name,Path(__file__).with_name(path));m=module_from_spec(s);s.loader.exec_module(m);return m
bridge=module('release_bridge','audit-concert-grand-moving-bridge.py')
join=module('release_join','render-concert-grand-exponential-note.py')
KEYS=['q','v','wave_state_final','relaxation_state_final']

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['input','asset','coefficients','baseline-report','kernel','parent-report','state-64','state-128','output','report']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--note',type=int,required=True);p.add_argument('--duration',type=float,default=.5)
    a=p.parse_args()
    if a.output.exists() or a.report.exists():raise ValueError('preserve measurements')
    start=time.monotonic();baseline=json.loads(a.baseline_report.read_text());parent=json.loads(a.parent_report.read_text())
    if not all(baseline['gates'].values()) or not all(parent['gates'].values()):raise ValueError('validated parent required')
    prov=baseline['provenance']
    if join.digest(a.asset)!=prov['asset_sha256'] or join.digest(a.coefficients)!=prov['coefficient_sha256']:raise ValueError('physical input mismatch')
    support,_=bridge.p2_support(a.asset,a.note,prov['support']['cutoff_hz']);system,_=bridge.load(a.input,a.note,124,support,prov['transverse'])
    factor,rate,mass,limit,_=load_prefix(a.coefficients,support.frequency);air=BoundedAir(factor,rate,mass,limit);del factor
    pilot=DampedCoast(system,air)
    with np.load(a.kernel) as d:kernel=d['kernel']
    result=dict(note=a.note,note_off_s=1.,duration_s=a.duration,damper=pilot.damper.parameters,ramp_s=.03,cases={},gates={},
        inputs_sha256={str(x):join.digest(x) for x in [a.input/'transfer.csv',a.input/'contact-end.csv',a.asset,a.coefficients,a.baseline_report,a.kernel,a.parent_report,a.state_64,a.state_128]},
        source_sha256={n:join.digest(Path(__file__).with_name(n)) for n in ['render-concert-grand-physical-release.py','concert_grand_damped_coast.py','concert_grand_polynomial_coast.py','concert_grand_exponential_coast.py']},
        scope='Distributed viscous damper approximation with assumed geometry/drag/lowering. All existing string, board and acoustic states continue. No output fade, room or tonal correction.')
    a.output.mkdir(parents=True);join.write(a.report,result);traces={};spectra={}
    for rate,path in [(64000,a.state_64),(128000,a.state_128)]:
        if a.note==60:expected=parent['cases'][str(rate)]['extension_sha256']
        else:expected=parent['coasts'][str(rate)]['sha256']
        if join.digest(path)!=expected:raise ValueError('parent state hash')
        with np.load(path) as d:state=tuple(d[k] for k in KEYS)
        initial=pilot.energy(state)['total_j'];initial_mechanical=system.string.energy(state[0][:-1],state[1][:-1])
        B=[pilot.board_map@state[0]];V=[pilot.board_map@state[1]];heat=material=port=relax=0.;last=now=time.monotonic();begin=now
        steps=round(a.duration*rate)
        for i in range(steps):
            state,s=pilot.release_step(state,1/rate,engagement((i+.5)/rate),True)
            heat+=s['damper_heat_j'];material+=s['material_loss_j'];port+=s['air_port_work_j'];relax+=s['radiation_loss_j']
            B.append(pilot.board_map@state[0]);V.append(pilot.board_map@state[1]);now=time.monotonic()
            if now-last>=25 or i+1==steps:
                print(f'note {a.note}, release {rate} Hz {(i+1)/steps:.1%}, dynamics {now-begin:.1f}s',flush=True);last=now
        final=pilot.energy(state)['total_j'];final_mechanical=system.string.energy(state[0][:-1],state[1][:-1])
        B=np.array(B);V=np.array(V);t=np.arange(len(B))/rate
        output=a.output/f'Release-{rate}.npz';np.savez_compressed(output,time_s=1+t,board_position=B,board_velocity=V,**dict(zip(KEYS,state)))
        metrics=dict(initial_energy_j=initial,final_energy_j=final,initial_string_board_energy_j=initial_mechanical,final_string_board_energy_j=final_mechanical,
            damper_heat_j=heat,board_material_heat_j=material,air_work_j=port,relaxation_heat_j=relax,
            radiation_inferred_j=initial-final-heat-material,dynamics_s=now-begin,sha256=join.digest(output))
        if rate==64000:
            h=CubicHermiteSpline(t,B,V,axis=0);t=np.arange(2*(len(B)-1)+1)/128000;B,V=h(t),h(t,1)
        _,f,P,_=pressure_fft(t,B,V,kernel);spectra[rate]=P;traces[rate]=(B,V)
        result['cases'][str(rate)]=metrics;join.write(a.report,result)
    b,v=traces[64000];B,V=traces[128000]
    comparison=dict(pressure_relative=spectral_relative(f,spectra[64000],spectra[128000],60,6000),position_relative=join.relative(b,B),velocity_relative=join.relative(v,V))
    result['comparison']=comparison
    result['gates']=dict(pressure=comparison['pressure_relative']<.01,board=max(comparison['position_relative'],comparison['velocity_relative'])<.01,
        positive_inferred_radiation=all(c['radiation_inferred_j']>=-1e-9*c['initial_energy_j'] for c in result['cases'].values()),
        passive_damper=all(c['damper_heat_j']>=0 and c['final_string_board_energy_j']<c['initial_string_board_energy_j'] for c in result['cases'].values()))
    result['all_gates_passed']=all(result['gates'].values());result['seconds']=time.monotonic()-start;join.write(a.report,result)
    if not result['all_gates_passed']:raise AssertionError(result['comparison'])
    print(json.dumps(result['gates']),flush=True)

if __name__=='__main__':main()
