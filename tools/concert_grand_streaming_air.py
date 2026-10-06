"""Paired Rayleigh FIR/correlation construction with bounded angular memory.

No angular channels or taps are discarded. The wave factor is regenerated
in blocks for its storage/loss certificate; only its modal correlations stay.
This saves H storage, not the physical accuracy gates or the dense R cost.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.sparse import coo_matrix
from scipy.fft import rfft,irfft,next_fast_len
from concert_grand_rational_air import RationalAir
from concert_grand_bounded_air import BoundedAir,RelaxingMass


class SurfaceFactorBlocks:
    def __init__(self,surface,rate,polar=12,azimuth=24,direction_block=8,smooth=True,fold_y=False):
        if (not np.isfinite(rate) or rate<=0 or polar<2 or azimuth<4
            or azimuth%2 or direction_block<1):raise ValueError('wave grid/block')
        self.surface=surface;self.rate=float(rate);self.block=int(direction_block);self.smooth=smooth
        self.modes=surface.modes
        g,w=leggauss(polar);u=(g+1)/2;w=w/2
        az=2*np.pi*(np.arange(azimuth//2)+.5)/azimuth;radial=np.sqrt(1-u*u)
        self.directions=np.column_stack((np.repeat(radial,len(az))*np.tile(np.cos(az),polar),
            np.repeat(radial,len(az))*np.tile(np.sin(az),polar)))
        self.weights=np.repeat(w,len(az))*2*np.pi/azimuth
        extent=(abs(self.directions[:,0])*surface.length+abs(self.directions[:,1])*surface.width)/surface.speed
        self.histogram_order=int(np.ceil(extent.max()*rate))+2
        self.taps=self.histogram_order+2+int(smooth);self.logical_outputs=2*len(self.directions)
        xx,yy=np.meshgrid((np.arange(surface.nx)+.5)*surface.hx-surface.length/2,
            (np.arange(surface.ny)+.5)*surface.hy-surface.width/2,indexing='ij')
        self.positions=np.column_stack((xx.ravel(),yy.ravel()))
        self.shapes=np.asarray(surface.shapes).reshape(-1,self.modes)
        if not np.isfinite(self.shapes).all():raise ValueError('finite surface shapes')
        self.fold_y=fold_y;self.symmetry_relative=None
        if fold_y:
            if azimuth%4:raise ValueError('folded grid requires azimuth divisible by four')
            shapes=self.shapes.reshape(surface.nx,surface.ny,self.modes)
            total=error=0.
            for first in range(0,surface.nx,8):
                part=shapes[first:first+8];difference=part-part[:,::-1]
                total+=float(np.einsum('ijk,ijk->',part,part));error+=float(np.einsum('ijk,ijk->',difference,difference))
            self.symmetry_relative=float(np.sqrt(error/max(total,1e-300)))
            if self.symmetry_relative>1e-12:raise ValueError('folding requires an even-y surface; no symmetrization')
            # Reflection in y identifies a second reversed pair. Its weight
            # is included exactly via sqrt(2), not dropped or renormalized.
            keep=np.tile(np.arange(len(az))<len(az)//2,polar)
            self.directions=self.directions[keep];self.weights=2*self.weights[keep]
        self.outputs=2*len(self.directions)

    def blocks(self):
        surface=self.surface;rate=self.rate;order=self.histogram_order;center=order/(2*rate)
        cells=np.arange(len(self.positions))
        for first in range(0,len(self.directions),self.block):
            count=min(self.block,len(self.directions)-first)
            H=np.zeros((self.taps,2*count,self.modes))
            for local,i in enumerate(range(first,first+count)):
                direction=self.directions[i];weight=self.weights[i]
                a,b=sorted((abs(direction[0])*surface.hx/surface.speed,abs(direction[1])*surface.hy/surface.speed))
                lower=center+self.positions@direction/surface.speed-(a+b)/2
                begin=np.floor(lower*rate+.5).astype(int)
                bins=begin[None,:]+np.arange(int(np.ceil((a+b)*rate))+2)[:,None]
                def cdf(z):
                    if a<1e-14:return np.clip(z/b,0.,1.)
                    z=np.clip(z,0.,a+b)
                    return np.where(z<a,z*z/(2*a*b),np.where(z<=b,(z-a/2)/b,1-(a+b-z)**2/(2*a*b)))
                left=(bins-.5)/rate-lower[None,:];probability=cdf(left+1/rate)-cdf(left)
                active=(bins>=0)&(bins<order+1)&(probability!=0)
                histogram=coo_matrix((probability[active],(bins[active],np.broadcast_to(cells,bins.shape)[active])),
                    shape=(order+1,len(cells))).tocsr()
                area=(histogram@self.shapes)*(surface.hx*surface.hy)
                scale=rate*np.sqrt(surface.density*weight/(2*np.pi**2*surface.speed))
                raw=np.diff(np.vstack((np.zeros((1,self.modes)),area,np.zeros((1,self.modes)))),axis=0)*scale
                if self.smooth:
                    H[:-1,2*local]+=raw/2;H[1:,2*local]+=raw/2
                else:H[:,2*local]=raw
                H[:,2*local+1]=-H[::-1,2*local]
            yield H

    def budget(self,frequency_block=16,row_block=16):
        n=self.modes;t=self.taps;fft=next_fast_len(2*t-1);f=fft//2+1;o=min(self.outputs,2*self.block)
        # Explicit ndarray upper estimate; BLAS/FFT/interpreter overhead is
        # measured separately. Gram and correlation are independent of angle count.
        return dict(full_factor_bytes=t*self.outputs*n*8,
            unfolded_factor_bytes=t*self.logical_outputs*n*8,
            gram_bytes=f*n*n*16,correlation_bytes=t*n*n*8,
            estimated_array_peak_bytes=(f*n*n*16+t*n*n*8+t*o*n*8+f*o*n*16
                +min(f,frequency_block)*n*n*16+fft*min(n,row_block)*n*8+self.shapes.nbytes),
            factor_block_bytes=t*o*n*8,fft_size=fft)


class StreamingRationalAir(RationalAir):
    def __init__(self,provider,max_array_bytes,frequency_block=16,row_block=16,progress=None):
        if frequency_block<1 or row_block<1:raise ValueError('positive frequency/row blocks')
        self.provider=provider;self.budget=provider.budget(frequency_block,row_block)
        if self.budget['estimated_array_peak_bytes']>max_array_bytes:raise MemoryError('declared wave array budget exceeded')
        self.rate=provider.rate;self.a=2*self.rate;self.order=provider.taps-1
        self.outputs=provider.outputs;self.modes=provider.modes;self.states=self.order*self.modes
        n=self.modes;size=self.budget['fft_size'];count=size//2+1
        gram=np.zeros((count,n,n),complex)
        for block,H in enumerate(provider.blocks()):
            F=rfft(H,n=size,axis=0)
            for first in range(0,count,frequency_block):
                part=F[first:first+frequency_block]
                gram[first:first+frequency_block]+=part.conj().swapaxes(1,2)@part
            if progress is not None:progress(block+1)
        # Bound inverse FFT temporaries instead of transforming the full Gram.
        del F,H
        self.R=np.empty((provider.taps,n,n))
        for first in range(0,n,row_block):
            self.R[:,first:first+row_block]=irfft(gram[:,first:first+row_block],n=size,axis=0)[:provider.taps]
        del gram
        square=error=0.
        for first in range(0,len(self.R),16):
            part=self.R[first:first+16];difference=part-part.swapaxes(1,2)
            square+=float(np.einsum('ijk,ijk->',part,part));error+=float(np.einsum('ijk,ijk->',difference,difference))
            self.R[first:first+16]=(part+part.swapaxes(1,2))/2
        scale=max(np.sqrt(square),1e-30);self.reciprocity_error=np.sqrt(error)/scale
        if self.reciprocity_error>1e-11:raise ValueError('paired reciprocal factors required')
        self.tail_mass=np.zeros((n,n));self.D=self.R[0]/2
        self.dc_error=float(np.linalg.norm(self.D+self.R[1:].sum(axis=0))/scale)
        if self.dc_error>1e-11:raise ValueError('wave factor must vanish at DC')

    def midpoint(self,state,velocity,dt):
        updated,force,inputs=self.midpoint_deferred(state,velocity,dt)
        return updated,force,float(self.radiation_batch(inputs[None])[0])

    def radiation_batch(self,inputs):
        inputs=np.asarray(inputs)
        if inputs.ndim!=3 or inputs.shape[1:]!=(self.order+1,self.modes):raise ValueError('batch wave inputs')
        result=np.zeros(len(inputs))
        for H in self.provider.blocks():
            field=np.einsum('tom,btm->bo',H,inputs,optimize=True)
            result+=np.sum(field*field,axis=1)/2
        return result

    def energy(self,state,velocity=None):
        state=np.asarray(state)
        if state.shape!=(self.order,self.modes):raise ValueError('cascade state')
        if not np.any(state):return 0.
        size=next_fast_len(2*self.order);S=rfft(state[::-1],n=size,axis=0)
        total=0.
        for H in self.provider.blocks():
            field=np.einsum('fom,fm->fo',rfft(H,n=size,axis=0),S,optimize=True)
            tails=irfft(field,n=size,axis=0)[self.order:2*self.order]
            total+=float(np.sum(tails*tails))
        return total/(2*self.rate)

    def with_static_tail(self,target):raise NotImplementedError('use finite-limit StreamingBoundedAir completion')
    def mass_congruent(self,target):raise NotImplementedError('no port congruence fit in streaming construction')


class StreamingBoundedAir(BoundedAir):
    def __init__(self,wave,target_mass,target_limit):
        self.wave=wave
        self.wave_limit=wave.D+np.einsum('k,kmn->mn',(-1.)**np.arange(1,wave.order+1),wave.R[1:])
        self.relaxation=RelaxingMass(np.asarray(target_mass)-wave.static_mass(),np.asarray(target_limit)-self.wave_limit)
        self.modes=wave.modes;self.states=wave.states+self.relaxation.states
