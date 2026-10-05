"""Exact modal coordinate adapter for projected mass/stiffness/full damping.

No reduction and no loss diagonalization. source displacement = basis @ eta;
source forces pull back with basis.T. Air and microphone maps must use this
same basis when the causal air model is built. This adapter supplies no air.
"""
from dataclasses import dataclass
import numpy as np
from scipy.linalg import eigh
from concert_grand_moving_bridge import Support


def symmetric_matrix(value,n,name):
    raw=np.asarray(value)
    if raw.shape!=(n,n) or np.iscomplexobj(raw) or not np.isfinite(raw).all():
        raise ValueError('finite real '+name+' matrix required')
    a=np.array(raw,dtype=float)
    scale=max(float(np.linalg.norm(a,2)),1e-30)
    if np.linalg.norm(a-a.T,2)>1e-12*scale:raise ValueError('symmetric '+name+' required')
    # Only remove antisymmetric floating point noise; never clip eigenvalues.
    return (a+a.T)/2


@dataclass(frozen=True)
class ModalSupportMap:
    frequency:np.ndarray
    basis:np.ndarray
    damping:np.ndarray

    @classmethod
    def from_matrices(cls,mass,stiffness,damping):
        n=len(mass)
        if not n:raise ValueError('nonempty projected board required')
        M=symmetric_matrix(mass,n,'mass');K=symmetric_matrix(stiffness,n,'stiffness')
        C=symmetric_matrix(damping,n,'damping')
        # Generalized eigensolve includes the actual projected mass.
        values,V=eigh(K,M)
        if values.min()<=0:raise ValueError('positive support stiffness required')
        D=V.T@C@V;D=(D+D.T)/2
        scale=max(float(np.linalg.norm(D,2)),1e-30)
        if np.linalg.eigvalsh(D).min() < -1e-12*scale:raise ValueError('passive support damping required')
        return cls(np.sqrt(values)/(2*np.pi),V,D)

    def support(self,source_force):
        b=np.asarray(source_force)
        if b.shape!=(len(self.frequency),) or np.iscomplexobj(b) or not np.isfinite(b).all():
            raise ValueError('one real projected board port required')
        return Support(tuple(self.frequency),tuple(self.basis.T@b),
            (0.,)*len(b),tuple(map(tuple,self.damping)))


def projected_matrices(model):
    """Assemble only the 606-coordinate mechanical matrices, not parent modes."""
    f=np.asarray(model['frequency']);n=len(f);low=int(model['low_count'])
    if not 0<low<n:raise ValueError('retained and projected tail required')
    M=np.eye(n);K=np.diag((2*np.pi*f)**2);C=np.zeros((n,n))
    M[low:,low:]=model['tail_mass'];K[low:,low:]=model['tail_stiffness']
    C[:low,:low]=np.diag(model['low_damping']);C[low:,low:]=model['tail_damping']
    return M,K,C
