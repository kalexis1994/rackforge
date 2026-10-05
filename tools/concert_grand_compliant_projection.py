"""Real congruence projection of a mass-normalized modal mechanical system.

Keep low modes exactly. Enrich its high sector with real/imaginary forced
displacements at explicit frequency shifts, including static flexibility.
Project mass, stiffness AND damping by the same map; no diagonal-loss fit,
no static feedthrough substituted for dynamic storage. Dry validation only.
"""
import numpy as np
from scipy.linalg import eigh


def build(frequency, loads, cutoff, shifts=(0., 3000., 6000.), loss=.046,
          rank_tolerance=1e-11):
    f=np.asarray(frequency,dtype=float);B=np.asarray(loads,dtype=float)
    shifts=np.asarray(shifts,dtype=float)
    if (f.ndim!=1 or len(f)<2 or B.ndim!=2 or B.shape[0]!=len(f)
        or B.shape[1]<1 or shifts.ndim!=1 or len(shifts)<1
        or not all(np.isfinite(x).all() for x in [f,B,shifts])
        or (f<=0).any() or (np.diff(f)<0).any() or (shifts<0).any()
        or not np.isfinite([cutoff,loss,rank_tolerance]).all()
        or cutoff<=0 or loss<=0 or not 0<rank_tolerance<1):
        raise ValueError('positive sorted mechanical spectrum, loads and shifts')
    low=np.flatnonzero(f<=cutoff);high=np.flatnonzero(f>cutoff)
    if not len(low) or not len(high):raise ValueError('nonempty retained and high sectors')
    wh=2*np.pi*f[high];columns=[]
    for shift in shifts:
        w=2*np.pi*shift
        displacement=B[high]/(wh[:,None]**2-w*w+1j*loss*w*wh[:,None])
        for part in [displacement.real,displacement.imag]:
            lengths=np.linalg.norm(part,axis=0);keep=lengths>0
            if keep.any():columns.append(part[:,keep]/lengths[keep])
    if not columns:raise ValueError('no high-sector excitation')
    snapshots=np.concatenate(columns,axis=1)
    Q,singular,_=np.linalg.svd(snapshots,full_matrices=False)
    rank=int(np.sum(singular>singular[0]*rank_tolerance));Q=Q[:,:rank]
    # Diagonalize projected stiffness relative to its ACTUAL projected mass.
    # The loss matrix generally does not share these eigenvectors.
    M=Q.T@Q;K=Q.T@((wh*wh)[:,None]*Q)
    values,rotation=eigh(K,M);Q=Q@rotation
    tail_mass=Q.T@Q
    tail_stiffness=Q.T@((wh*wh)[:,None]*Q)
    tail_damping=Q.T@((loss*wh)[:,None]*Q)
    projection=np.zeros((len(f),len(low)+rank))
    projection[low,np.arange(len(low))]=1
    projection[high,len(low):]=Q
    return dict(projection=projection,low_count=len(low),frequency=np.r_[f[low],np.sqrt(values)/(2*np.pi)],
        tail_mass=tail_mass,tail_stiffness=tail_stiffness,tail_damping=tail_damping,
        low_damping=loss*2*np.pi*f[low],loss=loss,shifts=shifts,
        normalized_snapshot_singular=singular,rank_tolerance=rank_tolerance,
        snapshot_columns=snapshots.shape[1],high_rank=rank)


def displacement(model,frequency,loads):
    """Projected-coordinate displacement per force, including dense tail loss."""
    f=np.asarray(frequency,dtype=float);B=np.asarray(loads,dtype=float)
    Q=model['projection'];n=model['low_count'];omega=2*np.pi*f
    if (f.ndim!=1 or B.ndim!=2 or B.shape[0]!=len(Q)
        or not np.isfinite(f).all() or not np.isfinite(B).all() or (f<0).any()):
        raise ValueError('frequency grid and full-coordinate port forces')
    projected=Q.T@B
    wl=2*np.pi*model['frequency'][:n]
    low=projected[None,:n]/(wl[None,:,None]**2-omega[:,None,None]**2
        +1j*omega[:,None,None]*model['low_damping'][None,:,None])
    dynamic=(model['tail_stiffness'][None]-omega[:,None,None]**2*model['tail_mass'][None]
        +1j*omega[:,None,None]*model['tail_damping'][None])
    rhs=np.broadcast_to(projected[n:],(len(f),)+projected[n:].shape)
    tail=np.linalg.solve(dynamic,rhs)
    return np.concatenate([low,tail],axis=1)


def mobility(model,frequency,loads):
    x=displacement(model,frequency,loads);B=model['projection'].T@loads
    return 2j*np.pi*np.asarray(frequency)[:,None,None]*np.einsum('mp,fmq->fpq',B,x)
