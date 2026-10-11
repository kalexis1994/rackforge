"""Observe accepted states without changing the frozen physical trajectory.

Solvers report their accepted middle state; the second damper half completes
the tuple. A rejected coast never advances the clock or a checkpoint. State
copies exist only at requested times, in RAM. No extra capsule on disk.
"""
import time
import numpy as np
from concert_grand_integrated_phrase import trajectory
from concert_grand_damper_control import DamperControl


class NoteTrace:
    def __init__(self,contact_rate,checkpoint_times):
        self.rate=contact_rate;self.tick=0;self.pending=None;self.states={}
        self.wanted={round(t*contact_rate):t for t in checkpoint_times}
        if any(k/contact_rate!=v for k,v in self.wanted.items()):
            raise ValueError('aligned checkpoint times required')
        self.air_work_j=0.

    def solver(self,pilot):
        tracker=self
        class Solver:
            def initial_state(self,*args):return pilot.initial_state(*args)
            def step(self,state,dt,defer_radiation=False):
                result,stats=pilot.step(state,dt,defer_radiation)
                if tracker.pending is not None:raise AssertionError('unconsumed accepted middle state')
                tracker.pending=(result,dt)
                tracker.air_work_j+=stats['air_port_work_j']
                return result,stats
        return Solver()

    def dampers(self,physical):
        tracker=self
        class Dampers:
            def dissipate(self,v,dt,levels):
                result,heat=physical.dissipate(v,dt,levels)
                if tracker.pending is not None:
                    middle,full_dt=tracker.pending
                    if dt!=full_dt/2:raise AssertionError('second physical damper half required')
                    tracker.tick+=round(full_dt*tracker.rate)
                    if tracker.tick in tracker.wanted:
                        state=(middle[0],result,middle[2],middle[3])
                        tracker.states[tracker.wanted[tracker.tick]]=tuple(x.copy() for x in state)
                    tracker.pending=None
                return result,heat
        return Dampers()

    def run(self,system,contact,coast,dampers,**args):
        result=trajectory(system,self.solver(contact),self.solver(coast),self.dampers(dampers),
                          contact_rate=self.rate,**args)
        if self.pending is not None or set(self.states)!=set(self.wanted.values()):
            raise AssertionError('missing accepted checkpoints')
        return result


def release_segment(coast,dampers,initial,start,duration,rate,key=60,note_off=.16):
    """Local refinement from the same live tuple during the actual release.

This diagnostic requires separated heads; recontact fails explicitly. It
uses the same two damper halves and absolute 30ms lowering ramp as the
complete renderer. No launch, velocity reset or added output envelope.
"""
    count=round(duration*rate)
    if count/rate!=duration:raise ValueError('aligned release interval required')
    control=DamperControl(coast.system.keys,False)
    control.event('on',key,0.);control.event('off',key,note_off)
    state=tuple(x.copy() for x in initial);core=coast.system.string
    def board(q):return core.board_transform@q[core.sb]
    B=[board(state[0])];V=[board(state[1])]
    begin=last=time.perf_counter();heat=work=material=0.
    for i in range(count):
        dt=1/rate;now=start+i/rate
        levels=control.levels(now+dt/2)
        v,h0=dampers.dissipate(state[1],dt/2,levels)
        middle,stats=coast.step((state[0],v,state[2],state[3]),dt,True)
        v,h1=dampers.dissipate(middle[1],dt/2,levels)
        state=(middle[0],v,middle[2],middle[3]);heat+=h0+h1
        work+=stats['air_port_work_j'];material+=stats['material_loss_j']
        B.append(board(state[0]));V.append(board(state[1]))
        if time.perf_counter()-last>25:
            print('note release control',rate,(i+1)/rate,'/',duration,flush=True);last=time.perf_counter()
    if not all(np.isfinite(x).all() for x in state):raise ValueError('nonfinite release state')
    return state,(np.arange(count+1)/rate,np.asarray(B),np.asarray(V)),dict(
        rate_hz=rate,seconds=time.perf_counter()-begin,duration_s=duration,
        absolute_start_s=start,damper_heat_j=heat,air_work_j=work,material_heat_j=material)
