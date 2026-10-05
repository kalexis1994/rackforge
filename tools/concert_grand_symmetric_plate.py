"""Exact reflection-invariant vertical subspace of the assumed P2 geometry.

Ribs parallel to y, centered straight bridge, clamped symmetric y edges,
vertical forces/readout on y=W/2 and zero initial antisymmetric motion.
Odd sine indices in y are symmetric; retain all x coordinates and all
frequencies in that invariant subspace. This is not a reduction for lateral
moments, off-center bridges/forces/microphones or an asymmetric real piano.
"""
import numpy as np
from scipy.linalg import eigh
import concert_grand_orthotropic_plate as plate

def assemble(panel,nx,ny,beams=(),boundary='clamped'):
    keep=np.arange(0,ny,2)
    mx,tx,kx=plate.integrals(panel.length,nx,boundary)
    full=plate.integrals(panel.width,ny,boundary);my,ty,ky=[a[np.ix_(keep,keep)] for a in full]
    dx,dy,d12,d66=panel.rigidities()
    mass=panel.density*panel.thickness*np.kron(mx,my)
    stiffness=dx*np.kron(kx,my)+dy*np.kron(mx,ky)+2*(d12+2*d66)*np.kron(tx,ty)
    for b in beams:
        if b.bow!=0:raise ValueError('centered straight bridge required')
        if b.axis=='y':
            p=plate.basis([b.position],panel.length,nx,boundary)[0][0];at=np.outer(p,p)
            mass+=b.line_mass*np.kron(at,my);stiffness+=b.rigidity*np.kron(at,ky)
        elif b.axis=='x':
            if b.position!=panel.width/2:raise ValueError('reflection symmetry of bridge required')
            p=plate.basis([b.position],panel.width,ny,boundary)[0][0,keep];at=np.outer(p,p)
            mass+=b.line_mass*np.kron(mx,at);stiffness+=b.rigidity*np.kron(kx,at)
        else:raise ValueError(b.axis)
    indices=(np.arange(nx)[:,None]*ny+keep).ravel()
    return mass,stiffness,indices

def solve(panel,nx,ny,beams=(),boundary='clamped'):
    mass,stiffness,indices=assemble(panel,nx,ny,beams,boundary)
    values,active=eigh(stiffness,mass)
    if values.min()<=0:raise ValueError('positive symmetric mechanical stiffness')
    vectors=np.zeros((nx*ny,len(values)));vectors[indices]=active
    return dict(panel=plate.asdict(panel),beams=[plate.asdict(b) for b in beams],boundary=boundary,nx=nx,ny=ny,
        frequency=np.sqrt(values)/(2*np.pi),vectors=vectors,active_vectors=active,mass=mass,stiffness=stiffness,indices=indices)
