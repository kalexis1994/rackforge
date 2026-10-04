"""Passive spectral realization of a baffled modal radiation load.

Positive matrix spectral weights become collocated oscillator ports. A
positive static tail accounts for frequencies beyond the quadrature cutoff.
The finite bath is an offline reference, not a compact real-time model.
Its broadening gamma is numerical absorption, not measured air/material loss.
"""
import numpy as np
from scipy.linalg import cho_factor,cho_solve


def positive_factor(matrix,tolerance=1e-10):
    """Remove round-off only; reject a materially indefinite spectral weight."""
    matrix=np.asarray(matrix,dtype=float)
    if matrix.ndim!=2 or matrix.shape[0]!=matrix.shape[1] or not np.isfinite(matrix).all():raise ValueError('finite square matrix')
    if not np.allclose(matrix,matrix.T,rtol=0.,atol=1e-12*max(np.linalg.norm(matrix),1e-30)):raise ValueError('reciprocal matrix required')
    values,vectors=np.linalg.eigh((matrix+matrix.T)/2);scale=max(np.max(abs(values)),1e-30)
    if values.min()<-tolerance*scale:raise ValueError('indefinite spectral weight/tail')
    keep=values>scale*1e-13
    factor=np.sqrt(values[keep])[:,None]*vectors[:,keep].T
    return factor,float(np.linalg.norm(factor.T@factor-matrix)/max(np.linalg.norm(matrix),1e-30))


class AcousticBath:
    def __init__(self,omega,gamma,projection,tail_mass):
        self.omega=np.asarray(omega,dtype=float);self.gamma=np.broadcast_to(gamma,self.omega.shape).astype(float).copy()
        self.projection=np.asarray(projection,dtype=float);self.tail_mass=np.asarray(tail_mass,dtype=float)
        if self.omega.ndim!=1 or self.projection.ndim!=2 or self.projection.shape[0]!=len(self.omega) or len(self.omega)==0:raise ValueError('bath dimensions')
        self.ports,self.modes=self.projection.shape
        if self.tail_mass.shape!=(self.modes,self.modes):raise ValueError('tail dimensions')
        if not np.isfinite(np.r_[self.omega,self.gamma,self.projection.ravel(),self.tail_mass.ravel()]).all() or (self.omega<=0).any() or (self.gamma<0).any():raise ValueError('passive bath coefficients')
        positive_factor(self.tail_mass)
        self.residue_groups=None

    @classmethod
    def from_spectrum(cls,frequency,resistance,static_mass,edges,gamma_hz):
        """Midpoint spectral quadrature: W = 4*R(f)*df (rad/s conversion).

        Z(s) = s*M_tail + sum W*s/(s^2+2*gamma*s+Omega^2).
        M_tail = M_static - sum W/Omega^2 must be PSD. Interpolate R/omega^2
        by positive linear combinations, retaining the quadratic DC limit.
        frequency[0] must be >0; DC limit is supplied as its extrapolated row.
        """
        frequency=np.asarray(frequency,dtype=float);resistance=np.asarray(resistance,dtype=float)
        edges=np.asarray(edges,dtype=float)
        if frequency.ndim!=1 or len(frequency)<2 or (frequency<=0).any() or (np.diff(frequency)<=0).any():raise ValueError('positive increasing spectrum')
        if resistance.shape!=(len(frequency),len(static_mass),len(static_mass)) or edges.ndim!=1 or edges[0]!=0 or (np.diff(edges)<=0).any() or edges[-1]>frequency[-1]:raise ValueError('spectral quadrature coverage')
        centers=(edges[:-1]+edges[1:])/2;weights=np.diff(edges);omega=2*np.pi*centers
        normalized=resistance/(2*np.pi*frequency[:,None,None])**2
        # All source spectral matrices must be passive before interpolation.
        source_correction=max(positive_factor(r)[1] for r in resistance)
        left=np.clip(np.searchsorted(frequency,centers)-1,0,len(frequency)-2)
        fraction=np.clip((centers-frequency[left])/(frequency[left+1]-frequency[left]),0,1)
        matrices=((1-fraction[:,None,None])*normalized[left]+fraction[:,None,None]*normalized[left+1])*omega[:,None,None]**2
        residues=4*weights[:,None,None]*matrices
        rows=[];frequencies=[];corrections=[];projected=[]
        for w,residue in zip(omega,residues):
            factor,correction=positive_factor(residue);corrections.append(correction)
            rows.append(factor);frequencies.extend([w]*len(factor));projected.append(factor.T@factor)
        residues=np.asarray(projected)
        tail=np.asarray(static_mass)-np.sum(residues/omega[:,None,None]**2,axis=0)
        tail_factor,tail_correction=positive_factor(tail)
        bath=cls(frequencies,2*np.pi*gamma_hz,np.vstack(rows),tail_factor.T@tail_factor)
        bath.residue_groups=(omega,residues)
        bath.construction=dict(segments=len(centers),ports=bath.ports,gamma_hz=gamma_hz,
            cutoff_hz=edges[-1],min_spacing_hz=float(weights.min()),max_spacing_hz=float(weights.max()),
            source_roundoff_relative=source_correction,spectral_roundoff_relative=max(corrections),tail_roundoff_relative=tail_correction,
            tail_mass_eigenvalues=np.linalg.eigvalsh(bath.tail_mass)[[0,-1]].tolist())
        return bath

    def impedance(self,s):
        if not np.isfinite(s):raise ValueError('finite Laplace frequency')
        if self.residue_groups is not None:
            omega,residues=self.residue_groups
            weight=s/(s*s+2*self.gamma[0]*s+omega**2)
            return s*self.tail_mass+np.einsum('p,pmn->mn',weight,residues)
        weight=s/(s*s+2*self.gamma*s+self.omega**2)
        return s*self.tail_mass+self.projection.T@(weight[:,None]*self.projection)

    def static_mass(self):return self.tail_mass+self.projection.T@((1/self.omega**2)[:,None]*self.projection)


class LoadedBoard:
    """Reciprocal mechanics/bath, implicit midpoint with a small Schur solve.

    M*v'= -K*q - D*v - G.T*p; bath q'=p, p'=G*v-Omega^2*q-2*gamma*p.
    M=I+M_tail. The tail's inertial energy belongs to air, not dry wood.
    At any dt>0, midpoint work equals stored-energy change plus positive loss.
    """
    def __init__(self,stiffness,material_damping,bath):
        self.bath=bath;self.K=np.asarray(stiffness,dtype=float);self.D=np.asarray(material_damping,dtype=float)
        n=bath.modes
        if self.K.shape!=(n,n) or self.D.shape!=(n,n):raise ValueError('board dimensions')
        positive_factor(self.K);positive_factor(self.D)
        if np.linalg.eigvalsh(self.K).min()<=0:raise ValueError('positive stiffness required')
        self.M=np.eye(n)+bath.tail_mass;self.mass_factor=cho_factor(self.M);self.cache=None

    def zero_state(self):return tuple(np.zeros(n) for n in (self.bath.modes,self.bath.modes,self.bath.ports,self.bath.ports))

    def energy(self,state):
        q,v,a,p=state
        dry=.5*(v@v+q@self.K@q);air=.5*(v@self.bath.tail_mass@v+np.dot(p,p)+np.dot(self.bath.omega*a,self.bath.omega*a))
        return dict(wood_j=float(dry),air_j=float(air),total_j=float(dry+air))

    def impulse(self,state,impulse):
        q,v,a,p=state;next_v=v+cho_solve(self.mass_factor,np.asarray(impulse))
        work=.5*float(np.asarray(impulse)@(v+next_v))
        return (q.copy(),next_v,a.copy(),p.copy()),work

    def derivative(self,state,force=None):
        q,v,a,p=state;force=np.zeros_like(v) if force is None else np.asarray(force)
        return (v,cho_solve(self.mass_factor,force-self.K@q-self.D@v-self.bath.projection.T@p),
                p,self.bath.projection@v-self.bath.omega**2*a-2*self.bath.gamma*p)

    def step(self,state,dt,force=None):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive timestep')
        q,v,a,p=state;h=dt/2;G=self.bath.projection;w=self.bath.omega;gamma=self.bath.gamma
        if self.cache is None or self.cache[0]!=dt:
            denominator=1+2*h*gamma+h*h*w*w
            reduced=self.M+h*self.D+h*h*self.K+h*h*(G.T@((1/denominator)[:,None]*G))
            self.cache=(dt,denominator,cho_factor(reduced))
        _,denominator,factor=self.cache
        free=(p-h*w*w*a)/denominator
        force=np.zeros_like(v) if force is None else np.asarray(force)
        mid_v=cho_solve(factor,self.M@v-h*self.K@q-h*(G.T@free)+h*force)
        mid_p=free+h/denominator*(G@mid_v)
        next_state=(q+dt*mid_v,2*mid_v-v,a+dt*mid_p,2*mid_p-p)
        stats=dict(material_loss_j=dt*float(mid_v@self.D@mid_v),
            bath_absorption_j=dt*float(np.dot(2*gamma,mid_p*mid_p)),
            external_work_j=dt*float(force@mid_v))
        acceleration=(next_state[1]-v)/dt
        stats['air_port_work_j']=dt*float(mid_v@(G.T@mid_p+self.bath.tail_mass@acceleration))
        return next_state,stats
