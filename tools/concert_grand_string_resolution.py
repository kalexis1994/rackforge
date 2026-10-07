"""Full contact capture and physical observations for string-resolution studies.

Mechanical work closes against the measured air port, not inferred acoustic
radiation. Full angular storage/flux is deliberately not certified here.
"""
import time
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from scipy.fft import irfft
from concert_grand_cell_observer import acceleration_spectrum,spectral_relative
from concert_grand_cell_audio import pressure_fft


BANDS=((60.,480.),(480.,1980.),(1980.,6000.),(60.,6000.))


def capture_contact(pilot,speed,rate,duration,progress=None):
    if (not isinstance(rate,int) or rate<=0 or not np.isfinite(duration) or duration<=0
            or abs(rate*duration-round(rate*duration))>1e-9 or round(rate*duration)<2):
        raise ValueError('positive rate and exact contact duration required')
    state=pilot.initial_state(speed);system=pilot.system
    initial=system.energy(state[0],state[1]);initial_hammer=float(state[1][-1])
    if initial<=0:raise ValueError('nonzero incident energy required')
    B=[pilot.board_map@state[0]];V=[pilot.board_map@state[1]]
    heat=dict(felt_heat_j=0.,material_heat_j=0.,relaxation_loss_j=0.,air_port_work_j=0.)
    aliases=dict(felt_heat_j='felt_loss_j',material_heat_j='material_loss_j',
                 relaxation_loss_j='radiation_loss_j',air_port_work_j='air_port_work_j')
    peak=impulse=budget=minimum=0.;iterations=fallbacks=0;seen=False;separations=[]
    steps=round(rate*duration);start=time.perf_counter()
    for i in range(steps):
        state,stats=pilot.step(state,1/rate,True)
        for key,field in aliases.items():heat[key]+=float(stats[field])
        compression=system.compression(state[0])
        if compression>0:seen=True
        elif seen:separations.append([i/rate,(i+1)/rate]);seen=False
        peak=max(peak,stats['force_n']);impulse+=stats['force_n']/rate
        minimum=min(minimum,stats['felt_loss_j'],stats['material_loss_j'],stats['radiation_loss_j'])
        balance=(system.energy(state[0],state[1])+heat['felt_heat_j']+heat['material_heat_j']
                 +heat['air_port_work_j']-initial)/initial
        budget=max(budget,abs(balance));iterations=max(iterations,stats['newton_iterations'])
        fallbacks+=stats['full_tangent_fallbacks']
        B.append(pilot.board_map@state[0]);V.append(pilot.board_map@state[1])
        if progress and (i+1)%max(1,steps//4)==0:progress(i+1,steps)
    momentum=np.sqrt(system.felt.hammer_mass)*(initial_hammer-state[1][-1])
    row=dict(rate=rate,steps=steps,duration_s=duration,dynamics_and_capture_s=time.perf_counter()-start,
        mechanical_initial_j=initial,mechanical_final_j=system.energy(state[0],state[1]),
        relaxation_final_j=pilot.air.relaxation.energy(state[3]),final_wave_norm=float(np.linalg.norm(state[2])),
        compression_final_m=system.compression(state[0]),maximum_mechanical_budget_relative=float(budget),
        minimum_step_heat_j=float(minimum),peak_centered_contact_force_n=float(peak),contact_impulse_n_s=float(impulse),
        hammer_momentum_change_n_s=float(momentum),impulse_momentum_relative=float(abs(impulse-momentum)/max(abs(momentum),1e-14)),
        separation_brackets_s=separations,newton_iterations_max=iterations,full_tangent_fallbacks=fallbacks,**heat)
    return state,row,np.arange(steps+1)/rate,np.array(B),np.array(V)


def join_capture(contact,coast):
    t,B,V=contact;tc,Bc,Vc=coast
    if not np.array_equal(B[-1],Bc[0]) or not np.array_equal(V[-1],Vc[0]):
        raise ValueError('contact/coast must share the same living physical endpoint')
    return np.r_[t,t[-1]+tc[1:]],np.vstack((B,Bc[1:])),np.vstack((V,Vc[1:]))


def pressure_observation(trace,frequency,transfer,kernel,pressure_rate):
    t,B,V=trace
    P=np.einsum('fcm,fm->fc',transfer,acceleration_spectrum(t,B,V,frequency))
    coarse=np.einsum('fcm,fm->fc',transfer,acceleration_spectrum(t[::2],B[::2],V[::2],frequency))
    common=np.arange(round(t[-1]*pressure_rate)+1)/pressure_rate
    spline=CubicHermiteSpline(t,B,V,axis=0);q,v=spline(common),spline(common,1)
    _,f,F,n=pressure_fft(common,q,v,kernel)
    band=irfft(np.where((f<=6000)[:,None],F,0)*pressure_rate,n=n,axis=0)
    norms={}
    for lo,hi in BANDS:
        mask=(frequency>=lo)&(frequency<=hi)
        norms[f'{lo:g}-{hi:g}']=float(2*np.trapezoid(np.sum(abs(P[mask])**2,axis=1),frequency[mask]))
    metadata=dict(pressure_peak_pa=np.max(abs(band),axis=0).tolist(),
        pressure_rms_pa=np.sqrt(np.mean(band*band,axis=0)).tolist(),
        positive_band_integral_pa2_s=norms,
        capture_decimation_relative=spectral_relative(frequency,coarse,P,60,6000),
        fft_size=n,source_endpoint_count=len(t))
    return (P,band,q,v),metadata


def compare_observation(actual,reference,frequency):
    P,p,q,v=actual;Pr,pr,qr,vr=reference
    relative=lambda a,b:float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))
    return dict(complex_pressure_relative={f'{lo:g}-{hi:g}':spectral_relative(frequency,P,Pr,lo,hi) for lo,hi in BANDS},
        temporal_pressure_relative=relative(p,pr),physical_board_position_relative=relative(q,qr),
        physical_board_velocity_relative=relative(v,vr))
