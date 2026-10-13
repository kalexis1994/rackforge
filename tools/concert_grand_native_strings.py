"""Native choir input and frozen Python reference with the same SI potential.

The bridge is fixed for this component test. Mixed endpoint inertia is
exported/checked separately; this is not a soundboard-coupled instrument.
"""
from pathlib import Path
import struct
import numpy as np
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_shared_strings import WirePort,SharedStrings,SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir


def fixture(note=60,transverse=12,rate=128000,velocity=84,seed=None,loss=.1,
            second_speed=0.,second_time=.012,duration=.018):
    length=.62*2**((60-note)/12)
    mu=.0081;young=200e9;density=7850.
    nt=transverse;nl=2*nt;nodes=1<<(max(nl,2*nt)+1).bit_length()
    cents=np.array([-.4,0.,.6]);tensions=850.*2**(cents/600)
    ports=[WirePort(60,Wire(length,mu,float(tension),young,density),[0.]) for tension in tensions]
    core=BatchedStrings(SharedStrings(ports,[110.],np.zeros((1,1)),nt,nl))
    system=SharedHammers(core,Felt(.002,7.5e8,2.6,.125,.02,loss))
    wave=PadeAir(RationalAir(np.zeros((3,2,1)),128000))
    air=PadeBoundedAir(wave,wave.static_mass()+np.eye(1)*.001,np.eye(1))
    reference=ExponentialContact(system,air)
    # Drop the unexcited dummy board coordinate from serialized states.
    keep=np.r_[np.arange(core.ns),system.size-1]
    q=np.zeros(len(keep));q[-1]=-.01*np.sqrt(.002);v=np.zeros_like(q)
    endpoint=q.copy()
    if seed is not None:
        rng=np.random.default_rng(seed)
        q[:-1]=rng.normal(size=core.ns)*2e-7
        endpoint[:-1]=q[:-1]+rng.normal(size=core.ns)*1e-7
        v[:-1]=rng.normal(size=core.ns)*.002
        for e in core.entries:
            q[e['su']]*=.03;endpoint[e['su']]*=.03
    shapes=np.concatenate([d[entry['sv']] for entry,(_,d,_) in zip(core.entries,system.contacts)])
    speed=5.*(velocity/124)**1.5
    return dict(note=note,transverse=nt,axial=nl,nodes=nodes,
        parameters=np.array([length,mu,young,density,rate,.002,7.5e8,2.6,loss,2e-5,-1e-5]),
        shapes=shapes,tensions=tensions,launch_speeds=np.array([speed,second_speed,0.]),
        times=np.array([0.,second_time,duration]),q=q,endpoint=endpoint,v=v,
        keep=keep,system=system,reference=reference)


def write_fixture(path,f):
    n=len(f['q'])
    with Path(path).open('xb') as stream:
        stream.write(struct.pack('<8sIII',b'RFSTR001',f['transverse'],f['axial'],f['nodes']))
        for key in ['parameters','shapes','tensions','launch_speeds','times','q','endpoint','v']:
            stream.write(np.asarray(f[key],dtype='<f8').tobytes())
    expected=20+8*(11+3*f['transverse']+9+3*n)
    if Path(path).stat().st_size!=expected:
        raise ValueError('native fixture byte count changed')


def replay(f):
    system,reference=f['system'],f['reference']
    q=np.zeros(system.size);v=q.copy()
    q[f['keep']]=f['q'];v[f['keep']]=f['v']
    initial=system.energy(q,v);heat=work=peak=0.;maximum_iterations=0;force=[];launches=[]
    rate=f['parameters'][4];steps=round(f['times'][2]*rate)
    for frame in range(steps):
        for time,speed in zip(f['times'][:2],f['launch_speeds'][:2]):
            if speed>0 and frame==round(time*rate):
                # Cold/live launch is the existing shared-head implementation.
                previous_q,previous_v=q.copy(),v.copy()
                (q,v,_,_),injected=system.launch((q,v,np.empty(0),np.empty(0)),60,speed)
                np.testing.assert_array_equal(q[:system.string.size],previous_q[:system.string.size])
                np.testing.assert_array_equal(v[:system.string.size],previous_v[:system.string.size])
                work+=injected;launches.append(injected)
        q,v,stats=reference.mechanical_step(q,v,1/rate)
        heat+=stats['felt_loss_j'];peak=max(peak,stats['force_n']);maximum_iterations=max(maximum_iterations,stats['mechanical_iterations'])
        if frame<round(.006*rate):force.append(stats['force_n'])
    return dict(q=q[f['keep']],v=v[f['keep']],heat_j=heat,launch_work_j=launches,
                energy_j=system.energy(q,v),force_peak_n=peak,maximum_iterations=maximum_iterations,
                force_trace=np.asarray(force),work_relative=abs(system.energy(q,v)+heat-initial-work)/max(abs(work),abs(initial),1e-30))
