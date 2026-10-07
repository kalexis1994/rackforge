"""Capture separated SI coast ports and close independent field intervals.

The wave radiation is supplied by a separate angular observer, never as a
remainder of mechanical work. No acoustic or mechanical state is reduced.
"""
import numpy as np
import time


def run_coast(pilot,initial,rate,duration,progress=None):
    if (not isinstance(rate,int) or rate<=0 or not np.isfinite(duration) or duration<=0
            or abs(rate*duration-round(rate*duration))>1e-9):
        raise ValueError('positive rate and exact duration required')
    state=tuple(x.copy() for x in initial)
    if pilot.system.compression(state[0])>=0:raise ValueError('separated initial hammer required')
    steps=round(rate*duration)
    if steps<2:raise ValueError('at least two coast steps required')
    pilot.oscillator_functions(1/(2*rate));pilot._air_prepare(1/rate)
    B=[pilot.board_map@state[0]];V=[pilot.board_map@state[1]]
    port=np.empty((steps,pilot.air.modes))
    totals=dict(felt_heat_j=0.,material_heat_j=0.,relaxation_loss_j=0.,air_port_work_j=0.)
    aliases={'felt_heat_j':'felt_loss_j','material_heat_j':'material_loss_j',
             'relaxation_loss_j':'radiation_loss_j','air_port_work_j':'air_port_work_j'}
    minimum=0.;iterations=fallbacks=0
    begin=time.perf_counter()
    for i in range(steps):
        state,stats=pilot.step(state,1/rate,True)
        port[i]=stats['wave_inputs'][0]
        for name,field in aliases.items():totals[name]+=stats[field]
        minimum=min(minimum,stats['material_loss_j'],stats['radiation_loss_j'],stats['felt_loss_j'])
        iterations=max(iterations,stats['mechanical_iterations']);fallbacks+=stats['full_tangent_fallbacks']
        B.append(pilot.board_map@state[0]);V.append(pilot.board_map@state[1])
        if progress and (i+1)%max(1,steps//4)==0:progress(i+1,steps)
    row=dict(rate=rate,steps=steps,duration_s=duration,dynamics_and_capture_s=time.perf_counter()-begin,
        mechanical_initial_j=pilot.system.energy(initial[0],initial[1]),
        mechanical_final_j=pilot.system.energy(state[0],state[1]),
        relaxation_initial_j=pilot.air.relaxation.energy(initial[3]),
        relaxation_final_j=pilot.air.relaxation.energy(state[3]),
        initial_wave_norm=float(np.linalg.norm(initial[2])),final_wave_norm=float(np.linalg.norm(state[2])),
        compression_final_m=pilot.system.compression(state[0]),minimum_step_heat_j=minimum,
        mechanical_iterations_max=iterations,full_tangent_fallbacks=fallbacks,**totals)
    return state,row,port,np.arange(steps+1)/rate,np.array(B),np.array(V)


def field_balance(row,before,after,radiation):
    """Worst independent-storage/flux endpoints; normalize to initial total."""
    for interval in [before,after,radiation]:
        lo,estimate,hi=[float(interval[k]) for k in ['lower_j','estimate_j','upper_j']]
        if not np.isfinite([lo,estimate,hi]).all() or not 0<=lo<=estimate<=hi:
            raise ValueError('finite ordered nonnegative field interval required')
    initial=row['mechanical_initial_j']+row['relaxation_initial_j']
    final=row['mechanical_final_j']+row['relaxation_final_j']
    heat=row['felt_heat_j']+row['material_heat_j']+row['relaxation_loss_j']
    e0=initial+before['estimate_j'];lower0=initial+before['lower_j']
    if lower0<=0:raise ValueError('positive initial total energy required')
    total=final+after['estimate_j']+heat+radiation['estimate_j']-e0
    air_constant=(row['relaxation_final_j']-row['relaxation_initial_j']
                  +row['relaxation_loss_j']-row['air_port_work_j'])
    lo=after['lower_j']-before['upper_j']+radiation['lower_j']
    hi=after['upper_j']-before['lower_j']+radiation['upper_j']
    return dict(wave_initial_j=before['estimate_j'],wave_final_j=after['estimate_j'],
        wave_radiation_j=radiation['estimate_j'],wave_initial_interval=before,
        wave_final_interval=after,wave_radiation_interval=radiation,
        total_budget_relative=abs(total)/e0,
        air_budget_relative=abs(after['estimate_j']-before['estimate_j']+radiation['estimate_j']+air_constant)/e0,
        total_budget_bound_relative=max(abs(final-initial+heat+lo),abs(final-initial+heat+hi))/lower0,
        air_budget_bound_relative=max(abs(air_constant+lo),abs(air_constant+hi))/lower0)
