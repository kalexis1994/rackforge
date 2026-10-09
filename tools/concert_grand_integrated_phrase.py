"""Live shared phrase with accelerated contact, unchanged keys and pedal.

The same living tuple passes between contact/coast/damper subflows. Event
times use integer ticks, and rejected recontact attempts never advance state.
Capture is physical board displacement/velocity; no tonal output correction.
"""
import time
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_damper_control import DamperControl


SCHEDULE = [(0.,'on',60,84),(.14,'off',60,0),(.25,'on',67,84),
            (.39,'off',67,0),(.50,'on',72,84),(.64,'off',72,0),
            (.75,'on',60,84),(.89,'off',60,0),(1.05,'pedal_off',0,0)]


def trajectory(system, contact, coast, dampers, pedal, duration=2.,
               contact_rate=128000, coast_rate=16000, schedule=SCHEDULE,
               progress=None):
    if contact_rate % coast_rate or contact_rate % 128000 or duration <= 0:
        raise ValueError('aligned positive rates and duration required')
    end = round(duration*contact_rate)
    if end/contact_rate != duration:
        raise ValueError('duration must align to contact ticks')
    events = []
    for when,kind,key,velocity in schedule:
        tick = round(when*contact_rate)
        if tick/contact_rate != when or not 0 <= tick < end:
            raise ValueError('event must align inside the trajectory')
        events.append((tick,kind,key,velocity))
    if events != sorted(events,key=lambda x:x[0]):
        raise ValueError('events must be chronological')
    state = contact.initial_state(0.)
    control = DamperControl(system.keys,pedal)
    stride = contact_rate//coast_rate; capture = contact_rate//128000
    core = system.string
    def board(q): return core.board_transform@q[core.sb]
    times=[0.]; B=[board(state[0])]; V=[board(state[1])]
    tick=cursor=contact_steps=coast_steps=retries=0
    felt=material=damper_heat=work=force=sympathetic=0.
    iterations=0; launches=[]; begin=last=time.perf_counter()
    while tick < end:
        now = tick/contact_rate
        while cursor < len(events) and events[cursor][0] == tick:
            _,kind,key,velocity=events[cursor]; cursor+=1
            control.event(kind,key,now)
            if kind == 'on':
                old=state
                state,external=system.launch(state,key,5.*(velocity/124)**1.5)
                for i in [0,1]:
                    if not np.array_equal(old[i][:core.size],state[i][:core.size]):
                        raise AssertionError('launch reset living mechanics')
                for i in [2,3]:
                    if not np.array_equal(old[i],state[i]):
                        raise AssertionError('launch reset living air')
                work+=external
                launches.append(dict(time_s=now,key=key,external_work_j=external))
        in_contact=system.compression(state[0]) >= -1e-12
        step=1 if in_contact or tick%stride else stride
        # A coarse step must finish before the next event and trajectory end.
        if tick+step > end or (cursor<len(events) and tick+step>events[cursor][0]):
            step=1
        dt=step/contact_rate; levels=control.levels(now+dt/2)
        v,heat0=dampers.dissipate(state[1],dt/2,levels)
        if in_contact or step==1:
            middle,stats=contact.step((state[0],v,state[2],state[3]),dt,True)
            contact_steps+=1
        else:
            try:
                middle,stats=coast.step((state[0],v,state[2],state[3]),dt,True)
                coast_steps+=1
            except ValueError as error:
                if 'recontact' not in str(error): raise
                # The attempt produced no accepted state. Recompute BOTH
                # damper halves for the refined duration from the old tuple.
                retries+=1; step=1; dt=1/contact_rate
                levels=control.levels(now+dt/2)
                v,heat0=dampers.dissipate(state[1],dt/2,levels)
                middle,stats=contact.step((state[0],v,state[2],state[3]),dt,True)
                contact_steps+=1
        v,heat1=dampers.dissipate(middle[1],dt/2,levels)
        state=(middle[0],v,middle[2],middle[3]); tick+=step
        damper_heat+=heat0+heat1; felt+=stats['felt_loss_j']; material+=stats['material_loss_j']
        force=max(force,stats['force_n']); iterations=max(iterations,stats.get('mechanical_iterations',0))
        if tick%capture==0 or tick==end:
            times.append(tick/contact_rate); B.append(board(state[0])); V.append(board(state[1]))
        if now < .24 and tick%stride==0 and 67 in system.keys:
            z=core.physical(state[0][:core.size])
            sympathetic=max(sympathetic,max(float(np.linalg.norm(z[e['sv']])/np.sqrt(e['port'].wire.modal_mass))
                           for e in core.entries if e['port'].key==67))
        if time.perf_counter()-last > 25:
            value=dict(physical_time_s=tick/contact_rate,duration_s=duration,
                       seconds=time.perf_counter()-begin,pedal=pedal,
                       contact_steps=contact_steps,coast_steps=coast_steps,recontact_retries=retries)
            print('integrated phrase',value,flush=True)
            if progress: progress(value)
            last=time.perf_counter()
    if cursor != len(events): raise AssertionError('missed physical event')
    if not all(np.isfinite(x).all() for x in state): raise ValueError('nonfinite final living state')
    spline=CubicHermiteSpline(times,np.asarray(B),np.asarray(V),axis=0)
    uniform=np.arange(round(duration*128000)+1)/128000
    metrics=dict(seconds=time.perf_counter()-begin,pedal=pedal,felt_heat_j=felt,
                 material_heat_j=material,damper_heat_j=damper_heat,external_head_work_j=work,
                 launches=launches,maximum_felt_force_n=force,max_mechanical_iterations=iterations,
                 contact_steps=contact_steps,coast_steps=coast_steps,recontact_retries=retries,
                 sympathetic_G4_before_strike_modal_norm_m=sympathetic,
                 final_mechanical_j=system.energy(*state[:2]),
                 radiation_ledger_deferred=True,complete_energy_certificate=False)
    return state,(uniform,spline(uniform),spline(uniform,1)),metrics
