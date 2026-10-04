"""Nonnegative matrix-residue compression of a known acoustic transfer.

Fits scalar weights of fixed PSD residue matrices, never recorded piano audio.
Static mass is preserved and its remaining tail must remain PSD. All channels
are collocated damped oscillators, so the existing energy identity still holds.
"""
import numpy as np
from scipy.linalg import cholesky,solve_triangular,expm
from scipy.optimize import minimize
from concert_grand_acoustic_bath import AcousticBath,positive_factor


def grouped(bath):
    """Exact grouping of equal-frequency/equal-loss oscillator channels."""
    pairs=np.column_stack((bath.omega,bath.gamma))
    unique,inverse=np.unique(pairs,axis=0,return_inverse=True)
    if np.any(np.diff(inverse)<0):
        order=np.argsort(inverse,kind='stable');projection=bath.projection[order];inverse=inverse[order]
    else:projection=bath.projection
    bounds=np.r_[0,np.flatnonzero(np.diff(inverse))+1,len(inverse)]
    residues=np.array([projection[a:b].T@projection[a:b] for a,b in zip(bounds,bounds[1:])])
    return unique[:,0],unique[:,1],residues


def linear_drive(bath,velocity,dt,stride=1):
    """Exact oscillator response to the SAME piecewise-linear port velocity.

    Augmented exponentials avoid low-frequency cancellation and also handle
    overdamped channels. Output includes inertial force on open intervals;
    a later prescribed velocity jump has a separate tail impulse.
    """
    pairs,inverse=np.unique(np.column_stack((bath.omega,bath.gamma)),axis=0,return_inverse=True)
    coefficients=[]
    for w,g in pairs:
        matrix=np.array([[0,1,0,0],[-w*w,-2*g,1,0],[0,0,0,1],[0,0,0,0]],dtype=float)
        coefficients.append(expm(dt*matrix)[:2])
    E=np.asarray(coefficients)[inverse];a=np.zeros(bath.ports);p=a.copy();forces=[np.zeros(bath.modes)]
    u0=bath.projection@velocity[0]
    for i in range(len(velocity)-1):
        u1=bath.projection@velocity[i+1];slope=(u1-u0)/dt
        na=E[:,0,0]*a+E[:,0,1]*p+E[:,0,2]*u0+E[:,0,3]*slope
        p=E[:,1,0]*a+E[:,1,1]*p+E[:,1,2]*u0+E[:,1,3]*slope;a=na;u0=u1
        if (i+1)%stride==0:
            forces.append(bath.projection.T@p+bath.tail_mass@((velocity[i+1]-velocity[i])/dt))
    return a,p,np.asarray(forces)


def free_force(bath,a,p,steps,dt):
    """Exact projected oscillator force after the prescribed port rests."""
    pairs,inverse=np.unique(np.column_stack((bath.omega,bath.gamma)),axis=0,return_inverse=True)
    order=np.argsort(inverse,kind='stable');projection=bath.projection[order];aa=a[order];pp=p[order];indices=inverse[order]
    bounds=np.r_[0,np.flatnonzero(np.diff(indices))+1,len(indices)]
    Q=np.array([projection[x:y].T@aa[x:y] for x,y in zip(bounds,bounds[1:])])
    P=np.array([projection[x:y].T@pp[x:y] for x,y in zip(bounds,bounds[1:])])
    E=np.array([expm(dt*np.array([[0,1],[-w*w,-2*g]])) for w,g in pairs])
    force=[P.sum(axis=0)]
    for _ in range(steps):
        next_Q=E[:,0,0,None]*Q+E[:,0,1,None]*P
        P=E[:,1,0,None]*Q+E[:,1,1,None]*P;Q=next_Q;force.append(P.sum(axis=0))
    return np.asarray(force)


def response(omega,gamma,residues,tail,s):
    s=np.atleast_1d(s).astype(complex)
    weights=s[:,None]/(s[:,None]**2+2*gamma[None,:]*s[:,None]+omega[None,:]**2)
    return s[:,None,None]*tail+np.einsum('fj,jmn->fmn',weights,residues,optimize=True)


def spectral_basis(data,edges,broadening):
    f=(edges[1:]+edges[:-1])/2;df=np.diff(edges);frequency=data['frequency_hz']
    normalized=data['resistance_s_inv']/(2*np.pi*frequency[:,None,None])**2
    left=np.clip(np.searchsorted(frequency,f)-1,0,len(frequency)-2)
    a=np.clip((f-frequency[left])/(frequency[left+1]-frequency[left]),0,1)
    matrices=(1-a[:,None,None])*normalized[left]+a[:,None,None]*normalized[left+1]
    # A_j = W_j / omega_j². W_j = 4 R(f_j) df_j, as in the frozen bath.
    A=4*df[:,None,None]*matrices
    for matrix in A:positive_factor(matrix)
    return 2*np.pi*f,2*np.pi*broadening*df,A


def truncate_basis(A,tolerance):
    """Discard weak positive spatial directions; transfer their static mass to tail."""
    if not np.isfinite(tolerance) or not 0<tolerance<1:raise ValueError('spatial tolerance')
    result=[];errors=[]
    for matrix in A:
        positive_factor(matrix);values,vectors=np.linalg.eigh(matrix)
        keep=values>max(values.max(),1e-30)*tolerance
        factor=np.sqrt(values[keep])[:,None]*vectors[:,keep].T
        projected=factor.T@factor;result.append(projected)
        errors.append(float(np.linalg.norm(projected-matrix)/max(np.linalg.norm(matrix),1e-30)))
    return np.asarray(result),max(errors)


def realize(omega,gamma,A,static_mass,weights):
    weights=np.asarray(weights)
    if weights.shape!=omega.shape or not np.isfinite(weights).all() or (weights<0).any():raise ValueError('nonnegative residue weights')
    tail=static_mass-np.einsum('j,jmn->mn',weights,A);tail=(tail+tail.T)/2
    positive_factor(tail)
    rows=[];frequencies=[];losses=[]
    for w,g,m,k in zip(omega,gamma,A,weights):
        factor,_=positive_factor(k*w*w*m)
        if len(factor):rows.append(factor);frequencies.extend([w]*len(factor));losses.extend([g]*len(factor))
    if not rows:raise ValueError('empty realization')
    bath=AcousticBath(frequencies,losses,np.vstack(rows),tail)
    bath.residue_groups=None # gamma varies between groups; use grouped response explicitly.
    return bath


def fit(omega,gamma,A,static_mass,s,target,regularization=1e-3):
    """Convex weighted least squares with k>=0 and sum(k A)<=M_static.

    Scalar residue weights preserve every matrix's spatial cross terms. A
    whitened maximum eigenvalue enforces the tail's PSD constraint. Failure
    is rejected rather than clipping its eigenvalues to make it passive.
    """
    s=np.asarray(s,dtype=complex);target=np.asarray(target,dtype=complex)
    if target.shape!=(len(s),len(static_mass),len(static_mass)):raise ValueError('target dimensions')
    factor=cholesky(static_mass,lower=True);inverse=solve_triangular(factor,np.eye(len(factor)),lower=True)
    whitened=np.einsum('ab,jbc,dc->jad',inverse,A,inverse,optimize=True)
    coefficient=s[:,None]*(omega[None,:]**2/(s[:,None]**2+2*gamma[None,:]*s[:,None]+omega[None,:]**2)-1)
    rhs=target-s[:,None,None]*static_mass
    scale_r=np.maximum(np.linalg.norm(target.real,axis=(1,2)),1e-30)
    scale_i=np.maximum(np.linalg.norm(target.imag,axis=(1,2)),1e-30)
    cr=coefficient.real/scale_r[:,None];ci=coefficient.imag/scale_i[:,None]
    br=rhs.real/scale_r[:,None,None];bi=rhs.imag/scale_i[:,None,None]
    spatial=np.einsum('jmn,kmn->jk',A,A,optimize=True)
    H=(cr.T@cr+ci.T@ci)*spatial
    r=np.einsum('fj,fmn,jmn->j',cr,br,A,optimize=True)+np.einsum('fj,fmn,jmn->j',ci,bi,A,optimize=True)
    normalization=max(float(np.trace(H)/len(H)),1e-30);H=H/normalization;r=r/normalization
    # Overlapping Lorentzian columns are poorly identifiable. A declared
    # ridge towards the physical quadrature prevents huge cancelling weights.
    if not np.isfinite(regularization) or regularization<0:raise ValueError('nonnegative ridge')
    H=H+regularization*np.eye(len(H));r=r+regularization
    def objective(k):return .5*k@H@k-r@k
    def gradient(k):return H@k-r
    def slack(k):
        value,vector=np.linalg.eigh(np.einsum('j,jmn->mn',k,whitened))
        v=vector[:,-1]
        return 1-1e-9-value[-1],-np.einsum('m,jmn,n->j',v,whitened,v)
    initial=np.ones(len(A));initial*=min(1.,.99/max(np.linalg.eigvalsh(whitened.sum(axis=0)).max(),1e-30))
    warm=np.maximum(np.linalg.solve(H,r),0.)
    result=minimize(objective,warm,jac=gradient,method='L-BFGS-B',bounds=[(0.,None)]*len(A),
                    options=dict(maxiter=10000,ftol=1e-15,gtol=1e-10,maxls=40))
    constrained=False
    if slack(result.x)[0]<0:
        constrained=True
        result=minimize(objective,initial,jac=gradient,method='SLSQP',bounds=[(0.,None)]*len(A),
            constraints=[dict(type='ineq',fun=lambda k:slack(k)[0],jac=lambda k:slack(k)[1])],
            options=dict(maxiter=1500,ftol=1e-11))
    if not result.success or slack(result.x)[0]<-1e-10:raise RuntimeError(f'passive compression did not converge: {result.message}')
    bath=realize(omega,gamma,A,static_mass,result.x)
    return bath,result.x,dict(iterations=int(result.nit),constrained_tail=constrained,
        whitened_tail_min_eigenvalue=float(slack(result.x)[0]+1e-9),optimizer_message=str(result.message),
        normalized_weight_ridge=regularization,objective='separately normalized real and imaginary transfer')
