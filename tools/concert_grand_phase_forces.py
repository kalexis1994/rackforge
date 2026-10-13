"""Independent dense-space/Gauss-time control for a free-carrier force kernel.

Only the quadratic gradient is integrated. This is not a nonlinear replay,
an energy-conserving step, a contact model, or a replacement for the teacher.
"""
import numpy as np
from scipy.special import roots_legendre


def quadratic_control(f, frames=16, order=1024, spatial_gauss=False):
    nt=f['transverse'];nl=f['axial'];width=nl+2*nt
    length,mu,young,density,rate=f['parameters'][:5]
    ea=young*mu/density;root=np.sqrt(mu*length/2)
    ku=np.pi*np.arange(1,nl+1)/length/root
    kv=np.pi*np.arange(1,nt+1)/length/root
    if spatial_gauss:
        points,weights=roots_legendre(16);cells=2*nt
        x=((np.arange(cells)[:,None]+(points[None]+1)/2)*length/cells).ravel()
        weight=np.tile(weights*length/(2*cells),cells)
    else:
        # Exact finite cosine quadrature for degree <= nl+2*nt < 2*nodes.
        nodes=f['nodes'];assert nl+2*nt<2*nodes
        x=(np.arange(nodes)+.5)*length/nodes
        weight=np.full(nodes,length/nodes)
    du=np.cos(x[:,None]*np.pi*np.arange(1,nl+1)/length)*ku
    dt=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*kv
    weighted_u=weight[:,None]*du;weighted_t=weight[:,None]*dt
    omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
    h=frames/rate;points,weights=roots_legendre(order)
    t=(points+1)*h/2;weights=weights*h/2
    quadratic=[];affine=[];end_forces=[]
    for wire in range(3):
        section=slice(wire*width,(wire+1)*width)
        w=omega[section];q=f['q'][section];v=f['v'][section]
        def point(qt):
            u=qt[...,:nl] @ du.T
            a=qt[...,nl:nl+nt] @ dt.T
            b=qt[...,nl+nt:] @ dt.T
            return ea*np.concatenate([(.5*(a*a+b*b)) @ weighted_u,
                (u*a) @ weighted_t,(u*b) @ weighted_t],axis=-1)
        qt=np.cos(t[:,None]*w)*q+np.sin(t[:,None]*w)*v/w
        gradient=point(qt)
        g0=point(q);g1=point(np.cos(h*w)*q+np.sin(h*w)*v/w)
        linear=g0+(g1-g0)*(t/h)[:,None]
        response_v=np.cos((h-t[:,None])*w)*weights[:,None]
        response_q=np.sin((h-t[:,None])*w)/w*weights[:,None]
        for force,out in [(gradient,quadratic),(linear,affine)]:
            out.append((-np.sum(response_q*force,axis=0),-np.sum(response_v*force,axis=0)))
        end_forces.extend(g1)
    def combine(values):
        q=np.concatenate([a for a,_ in values]);v=np.concatenate([b for _,b in values])
        return dict(dq=q,dv=v,metric=np.r_[omega*q,v])
    return dict(exact=combine(quadratic),affine=combine(affine),omega=omega,
                endpoint_quadratic_gradient=np.asarray(end_forces))


def response_metric(result,omega):
    return np.r_[omega*np.asarray(result['dq']),result['dv']]


def relative(actual,expected):
    return float(np.linalg.norm(actual-expected)/max(np.linalg.norm(expected),1e-30))
