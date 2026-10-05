"""Retarded pressure kernel in the plate's separable Ritz coordinates.

Project the very same cell histogram as CellObserver/retarded_kernel, using
two smaller tensor contractions. No surface FFT or cell-by-Ritz matrix is
required when only microphone readout is wanted. These coordinates are not
mass normalized; multiplying by physical modal eigenvectors supplies that.
"""
from types import SimpleNamespace
import numpy as np
from concert_grand_surface_impedance import average_basis
from concert_grand_cell_observer import CellObserver


def basis_kernel(panel,nx,ny,boundary,cells,rate,microphones,order=4):
    cx,cy=cells;length,width=panel['length'],panel['width']
    surface=SimpleNamespace(nx=cx,ny=cy,hx=length/cx,hy=width/cy,modes=1,
        density=1.2,speed=343.,shapes=np.ones((cx,cy,1)))
    observer=CellObserver(surface,microphones,order)
    bx=average_basis(length,nx,np.linspace(0,length,cx+1),boundary)
    by=average_basis(width,ny,np.linspace(0,width,cy+1),boundary)
    delay=observer.distance/surface.speed*rate
    if delay.min()<1:raise ValueError('positive propagation delay required')
    base=np.floor(delay).astype(int);u=delay-base
    weights=[-u*(u-1)*(u-2)/6,(u+1)*(u-1)*(u-2)/2,-(u+1)*u*(u-2)/2,(u+1)*u*(u-1)/6]
    size=int(base.max())+3;result=np.zeros((size,len(microphones),nx,ny))
    indices=np.tile(np.arange(cx*cy),len(observer.area))
    for mic in range(len(microphones)):
        histogram=np.zeros((size,cx*cy))
        for shift,w in zip([-1,0,1,2],weights):
            coefficients=observer.area[:,None]*w[:,mic]/observer.distance[:,mic]
            np.add.at(histogram,((base[:,mic]+shift).ravel(),indices),coefficients.ravel())
        left=np.einsum('kxy,xa->kay',histogram.reshape(size,cx,cy),bx,optimize=True)
        result[:,mic]=surface.density/(2*np.pi)*np.einsum('kay,yb->kab',left,by,optimize=True)
    return result
