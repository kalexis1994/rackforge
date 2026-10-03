"""P2 structural pilot: orthotropic Kirchhoff plate + discrete bending beams.

Rayleigh-Ritz with separable clamped or simply supported functions. Positive
line mass and centroidal EI only: no eccentric membrane/composite correction,
rib torsion, crown/prestress, irregular outline or measured Steinway geometry.
SI units. Generalized eigenvectors are normalized in the assembled mass.
"""
from dataclasses import dataclass, asdict

import numpy as np
from scipy.linalg import eigh
from numpy.polynomial.legendre import leggauss


@dataclass(frozen=True)
class Panel:
    length: float = 1.4
    width: float = 1.0
    thickness: float = .008
    density: float = 392
    ex: float = 11.5e9
    ey: float = .47e9
    gxy: float = .5e9
    nuxy: float = .3

    def rigidities(self):
        nuyx=self.nuxy*self.ey/self.ex
        factor=self.thickness**3/(12*(1-self.nuxy*nuyx))
        return self.ex*factor,self.ey*factor,self.nuxy*self.ey*factor,self.gxy*self.thickness**3/12


@dataclass(frozen=True)
class Beam:
    axis: str
    position: float
    width: float
    height: float
    young: float
    density: float
    bow: float = 0.0

    @property
    def line_mass(self):return self.density*self.width*self.height

    @property
    def rigidity(self):return self.young*self.width*self.height**3/12


def basis(x,length,count,boundary):
    """Return phi, first derivative and curvature; each row is a point."""
    x=np.asarray(x)
    k=np.pi*np.arange(1,count+1)/length
    if boundary=='simply-supported':
        s=np.sin(x[:,None]*k); c=np.cos(x[:,None]*k)
        return s,c*k,-s*k*k
    if boundary!='clamped':raise ValueError(boundary)
    # sin(m*pi*x/L)*sin(pi*x/L) satisfies displacement and slope zero
    # at both edges. A smooth nested Ritz space, not individual eigenfunctions.
    q=np.pi/length
    s=np.sin(x[:,None]*k); c=np.cos(x[:,None]*k)
    u=np.sin(x[:,None]*q); v=np.cos(x[:,None]*q)
    return s*u,c*k*u+s*v*q,-s*u*(k*k+q*q)+2*c*v*k*q


def integrals(length,count,boundary):
    nodes,weights=leggauss(max(64,4*count))
    x=(nodes+1)*length/2; w=weights*length/2
    p,d,c=basis(x,length,count,boundary)
    return p.T@(w[:,None]*p),d.T@(w[:,None]*d),c.T@(w[:,None]*c)


def assemble(panel,nx,ny,beams=(),boundary='clamped'):
    mx,tx,kx=integrals(panel.length,nx,boundary)
    my,ty,ky=integrals(panel.width,ny,boundary)
    dx,dy,d12,d66=panel.rigidities()
    mass=panel.density*panel.thickness*np.kron(mx,my)
    stiffness=dx*np.kron(kx,my)+dy*np.kron(mx,ky)+2*(d12+2*d66)*np.kron(tx,ty)
    for beam in beams:
        if min(beam.line_mass,beam.rigidity)<=0:raise ValueError('beam mass/EI must be positive')
        if beam.axis=='y':
            if not 0<beam.position<panel.length:raise ValueError('rib outside panel')
            p,_,_=basis([beam.position],panel.length,nx,boundary)
            at=np.outer(p[0],p[0])
            mass+=beam.line_mass*np.kron(at,my)
            stiffness+=beam.rigidity*np.kron(at,ky)
        elif beam.axis=='x':
            if not 0<beam.position<panel.width:raise ValueError('bridge outside panel')
            if beam.bow == 0:
                # Preserve the existing straight-beam assembly exactly.
                p,_,_=basis([beam.position],panel.width,ny,boundary)
                at=np.outer(p[0],p[0])
                mass+=beam.line_mass*np.kron(mx,at)
                stiffness+=beam.rigidity*np.kron(kx,at)
            else:
                bm,bk=curved_beam_matrices(panel,nx,ny,beam,boundary)
                mass+=bm
                stiffness+=bk
        else:raise ValueError(beam.axis)
    return mass,stiffness


def beam_path(x,panel,beam):
    """Assumed planar centreline y(x), y'(x), y''(x), SI units."""
    if beam.axis!='x':raise ValueError('curvature pilot supports longitudinal beam only')
    q=np.pi/panel.length
    x=np.asarray(x)
    return (beam.position+beam.bow*np.sin(q*x),
            beam.bow*q*np.cos(q*x),-beam.bow*q*q*np.sin(q*x))


def curved_beam_matrices(panel,nx,ny,beam,boundary,quadrature=None):
    """Positive Gram mass/flexion matrices for a bending-only curved line.

    Restrict the plate deflection to the actual centreline and differentiate
    twice with respect to arc length. This reduced pilot omits curved-rod
    torsion, eccentric/composite action and in-plane response.
    """
    nodes,weights=leggauss(quadrature or max(256,6*(nx+ny)))
    x=(nodes+1)*panel.length/2
    y,yp,ypp=beam_path(x,panel,beam)
    if not ((y>0)&(y<panel.width)).all():raise ValueError('curved bridge outside panel')
    px,dx,cx=basis(x,panel.length,nx,boundary)
    py,dy,cy=basis(y,panel.width,ny,boundary)
    product=lambda a,b:np.einsum('pm,pn->pmn',a,b).reshape(len(x),-1)
    phi=product(px,py)
    wx=product(dx,py)+yp[:,None]*product(px,dy)
    wxx=(product(cx,py)+2*yp[:,None]*product(dx,dy)
         +yp[:,None]**2*product(px,cy)+ypp[:,None]*product(px,dy))
    metric=1+yp*yp
    curvature=wxx/metric[:,None]-wx*(yp*ypp/metric**2)[:,None]
    ds=weights*panel.length/2*np.sqrt(metric)
    return (beam.line_mass*(phi.T@(ds[:,None]*phi)),
            beam.rigidity*(curvature.T@(ds[:,None]*curvature)))


def solve(panel,nx,ny,beams=(),boundary='clamped',modes=None):
    mass,stiffness=assemble(panel,nx,ny,beams,boundary)
    subset=None if modes is None else (0,min(nx*ny,modes)-1)
    values,vectors=eigh(stiffness,mass,subset_by_index=subset,driver='gvx' if subset else 'gvd')
    assert values.min()>0
    return dict(panel=asdict(panel),beams=[asdict(b) for b in beams],boundary=boundary,
                nx=nx,ny=ny,frequency=np.sqrt(values)/(2*np.pi),vectors=vectors,
                mass=mass,stiffness=stiffness)


def shape(model,x,y,derivative=None):
    x,y=np.broadcast_arrays(x,y)
    a=basis(x.ravel(),model['panel']['length'],model['nx'],model['boundary'])
    b=basis(y.ravel(),model['panel']['width'],model['ny'],model['boundary'])
    px=a[1 if derivative=='x' else 0]; py=b[1 if derivative=='y' else 0]
    rows=np.einsum('pm,pn->pmn',px,py).reshape(len(px),-1)
    return rows@model['vectors']


def mobility(model,frequency,points,loss=.046):
    """Reciprocal point velocity/force matrix, exp(+i omega t) convention."""
    omega=2*np.pi*np.asarray(frequency)
    w0=2*np.pi*model['frequency']
    phi=shape(model,np.array([p[0] for p in points]),np.array([p[1] for p in points]))
    response=1j*omega[:,None]/(w0[None,:]**2-omega[:,None]**2+1j*loss*w0[None,:]*omega[:,None])
    return np.einsum('pm,fm,qm->fpq',phi,response,phi)


def pilot_beams(panel,include_ribs=True,include_bridge=True):
    # Slight geometric irregularity fixed explicitly, not random per mode/note.
    fractions=np.array([.10,.205,.305,.407,.503,.607,.708,.802,.902])
    heights=np.array([.020,.021,.020,.022,.021,.020,.022,.021,.020])
    result=[]
    if include_ribs:
        result.extend(Beam('y',float(x*panel.length),.020,float(h),panel.ex,panel.density)
                      for x,h in zip(fractions,heights))
    if include_bridge:
        result.append(Beam('x',.5*panel.width,.030,.040,10e9,660))
    return tuple(result)
