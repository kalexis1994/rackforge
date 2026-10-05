"""Finite-width alternative to P2's delta-line ribs and straight bridge.

The same integrated positive line mass/EI is averaged over the declared
width, as parallel longitudinal bending fibres. This is a changed physical
idealization, not a numerical refinement of the delta-line model. It still
omits eccentric/composite membrane effects, torsion and crown/prestress.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.linalg import eigh
import concert_grand_orthotropic_plate as plate

def strip_gram(length,count,boundary,center,width):
    if width<0 or not 0<center<length or center-width/2<0 or center+width/2>length:raise ValueError('interior finite strip')
    if width==0:
        p=plate.basis([center],length,count,boundary)[0][0];return np.outer(p,p)
    nodes,weights=leggauss(max(32,2*int(np.ceil(count*width/length))))
    p=plate.basis(center+width*nodes/2,length,count,boundary)[0]
    return p.T@((weights/2)[:,None]*p)

def assemble(panel,nx,ny,beams=(),boundary='clamped',spread=1.):
    if not 0<=spread<=1:raise ValueError('footprint fraction')
    mass,stiffness=plate.assemble(panel,nx,ny,(),boundary)
    mx,_,kx=plate.integrals(panel.length,nx,boundary);my,_,ky=plate.integrals(panel.width,ny,boundary)
    for beam in beams:
        if beam.bow!=0:raise ValueError('straight finite-strip control only')
        if beam.axis=='y':
            at=strip_gram(panel.length,nx,boundary,beam.position,spread*beam.width)
            mass+=beam.line_mass*np.kron(at,my);stiffness+=beam.rigidity*np.kron(at,ky)
        elif beam.axis=='x':
            at=strip_gram(panel.width,ny,boundary,beam.position,spread*beam.width)
            mass+=beam.line_mass*np.kron(mx,at);stiffness+=beam.rigidity*np.kron(kx,at)
        else:raise ValueError(beam.axis)
    return mass,stiffness

def solve(panel,nx,ny,beams=(),boundary='clamped',modes=None):
    mass,stiffness=assemble(panel,nx,ny,beams,boundary)
    subset=None if modes is None else (0,min(nx*ny,modes)-1)
    values,vectors=eigh(stiffness,mass,subset_by_index=subset,driver='gvx' if subset else 'gvd')
    if values.min()<=0:raise ValueError('positive structural stiffness required')
    return dict(panel=plate.asdict(panel),beams=[plate.asdict(b) for b in beams],boundary=boundary,
        nx=nx,ny=ny,frequency=np.sqrt(values)/(2*np.pi),vectors=vectors,mass=mass,stiffness=stiffness)
