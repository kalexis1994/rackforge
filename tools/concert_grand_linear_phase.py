"""Small continuous linear bank and coupled phase controls, not a piano fit.

Energy coordinates include the complete positive acoustic storage. Heads
are parked and omitted only from this separated, homogeneous linear map.
Gauss4 is applied to the coupled block; dry carrier blocks remain exact.
No production R cache, temporal history or plugin is changed.
"""
import numpy as np
from scipy.linalg import block_diag,eigh,expm,solve
from concert_grand_shared_strings import WirePort,SharedStrings,SharedHammers
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast,PackedResidues
from concert_grand_port_avf_coast import PortAVFCoast


def gauss4_maps(L,dt):
    """Linear two-stage Gauss collocation, including both stage states."""
    if dt<=0 or not np.isfinite(dt): raise ValueError('positive finite interval')
    I=np.eye(len(L)); hL=dt*L
    D=I-hL/2+hL@hL/12
    g=np.sqrt(3)/6
    Y1=solve(D,I-g*hL,check_finite=False)
    Y2=solve(D,I+g*hL,check_finite=False)
    return I+hL@(Y1+Y2)/2,(Y1,Y2)


class LinearBank:
    def __init__(self,transverse=24,axial=48):
        ports=[WirePort(k,Wire(length,.0081,850.),[1.7,-.9,.4])
               for k,length in [(60,.62),(67,.51),(72,.31)]]
        core=SharedStrings(ports,[110.,270.,680.],np.diag([.01,.02,.03]),
                           transverse,axial,False)
        rng=np.random.default_rng(7); part=rng.normal(size=(7,3,3))*.001
        part-=part.mean(axis=0); H=np.empty((7,6,3))
        H[:,::2]=part; H[:,1::2]=-part[::-1]
        wave=PadeAir(RationalAir(H,128000))
        air=PadeBoundedAir(wave,wave.static_mass()+np.eye(3)*.001,np.eye(3))
        self.air=project_air_preserving_relaxation(air,np.eye(3)[:,:2])
        system=SharedHammers(BatchedStrings(core),Felt(.002,7.5e8,2.6,.125,.02,.1))
        residues=PackedResidues(self.air.wave)
        self.pilots=dict(split=PackedCoast(system,self.air,residues),
                         port=PortAVFCoast(system,self.air,residues))
        self.system=system; self.core=system.string; self.n=self.core.size
        zero=self.air.zero_state(); self.shape=zero[0].shape
        self.nr=zero[0].size; self.nz=zero[1].size
        self.size=2*self.n+self.nr+self.nz
        self.C=self.pilots['split'].C[:,:self.n]
        # Polarization of the original energy, no spectral clipping.
        na=self.nr+self.nz; units=np.eye(na)
        energy=lambda x:self.air.energy((x[:self.nr].reshape(self.shape),x[self.nr:]))
        diagonal=np.array([2*energy(x) for x in units]); Wa=np.diag(diagonal)
        for i in range(na):
            for j in range(i):
                Wa[i,j]=Wa[j,i]=energy(units[i]+units[j])-(diagonal[i]+diagonal[j])/2
        def roots(W):
            values,basis=eigh(W)
            if values[0]<=0: raise ValueError('strict positive energy, no repair')
            return (basis*np.sqrt(values))@basis.T,(basis/np.sqrt(values))@basis.T
        # Preserve the exact structural zeros between independent carriers.
        # A global eigensolve may numerically mix degenerate dry blocks.
        rk=np.zeros_like(self.core.K); ik=np.zeros_like(self.core.K)
        for group in self.core.groups:
            rblock,iblock=roots(self.core.K[np.ix_(group,group)])
            rk[np.ix_(group,group)]=rblock; ik[np.ix_(group,group)]=iblock
        ra,ia=roots(Wa)
        self.root=block_diag(rk,np.eye(self.n),ra)
        self.inverse=block_diag(ik,np.eye(self.n),ia)
        # Continuous RHS is independent of all discrete integration methods.
        raw=np.column_stack([self.rhs_raw(e) for e in np.eye(self.size)])
        self.L=self.root@raw@self.inverse
        group=next(g for g in self.core.groups if np.any(g>=self.core.ns))
        self.coupled=np.r_[group,self.n+group,np.arange(2*self.n,self.size)]
        self.dry=np.setdiff1d(np.arange(self.size),self.coupled)
        if max(np.linalg.norm(self.L[np.ix_(self.coupled,self.dry)]),
               np.linalg.norm(self.L[np.ix_(self.dry,self.coupled)]))>1e-9:
            raise ValueError('unexpected cross coupling')
        self.Lc=self.L[np.ix_(self.coupled,self.coupled)]

    def unpack_raw(self,y):
        n=self.n
        return y[:n],y[n:2*n],y[2*n:2*n+self.nr].reshape(self.shape),y[2*n+self.nr:]

    def rhs_raw(self,y):
        q,v,r,z=self.unpack_raw(y)
        (dr,dz),force=self.air.derivative((r,z),self.C@v)
        return np.r_[v,-self.core.K@q-self.core.damping@v-self.C.T@force,dr.ravel(),dz]

    def state(self,x):
        q,v,r,z=self.unpack_raw(self.inverse@x)
        return np.r_[q,np.full(self.system.nh,-.01)],np.r_[v,np.zeros(self.system.nh)],r,z

    def pack(self,state):
        return self.root@np.r_[state[0][:self.n],state[1][:self.n],state[2].ravel(),state[3]]

    def powers(self,x):
        _,v,r,z=self.unpack_raw(self.inverse@x); boundary=self.C@v
        wave=self.air.wave; inputs=[wave.port.T@boundary]
        for row in r: inputs.append(inputs[-1]-12*row[1])
        radiation=float(wave.inner.radiation_batch(np.array(inputs)[None])[0])
        d=self.air.relaxation.B.T@boundary-z
        return dict(material=float(v@self.core.damping@v),relaxation=float(d@d),wave=radiation)

    def actual_step(self,label,x,dt):
        state,_=self.pilots[label].step(self.state(x),dt,True)
        return self.pack(state)

    def transition(self,label,dt,amplitude=1e-7):
        if label in self.pilots:
            return np.column_stack([self.actual_step(label,amplitude*e,dt)/amplitude
                                    for e in np.eye(self.size)])
        if label=='exact': return expm(dt*self.L)
        if label=='gauss4':
            result=np.zeros_like(self.L)
            result[np.ix_(self.coupled,self.coupled)]=gauss4_maps(self.Lc,dt)[0]
            result[np.ix_(self.dry,self.dry)]=expm(dt*self.L[np.ix_(self.dry,self.dry)])
            return result
        raise ValueError('known phase control required')

    def coupled_transition(self,label,dt,amplitude=1e-7):
        if label in self.pilots:
            columns=[]
            for index in self.coupled:
                x=np.zeros(self.size);x[index]=amplitude
                columns.append(self.actual_step(label,x,dt)[self.coupled]/amplitude)
            return np.column_stack(columns)
        if label=='exact': return expm(dt*self.Lc)
        if label=='gauss4': return gauss4_maps(self.Lc,dt)[0]
        raise ValueError('known coupled phase control required')
