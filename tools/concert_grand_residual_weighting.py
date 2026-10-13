"""Frozen training-residual importance for resumed variable projection.

Importance changes identification weights, never microphone gain. The readout
scale stays in the previous raw coordinates, preserving the same ridge.
"""
import numpy as np
from concert_grand_variable_projection import VariablePressureProblem


def residual_importance(frequency,error_y,error_p,previous_indices,bins=24):
    f = np.asarray(frequency)
    if (f.ndim!=1 or len(f)<2 or np.any(np.diff(f)<=0) or
            error_y.shape!=f.shape or error_p.shape!=f.shape or
            not np.isfinite(error_y).all() or not np.isfinite(error_p).all()):
        raise ValueError('finite ordered training residuals required')
    boost_y = np.clip((error_y/.07)**2,1.,8.)
    boost_p = np.clip((error_p/.05)**2,1.,8.)
    worst = []
    edges = np.linspace(f[0],f[-1],bins+1)
    for first,last in zip(edges[:-1],edges[1:]):
        indices = np.flatnonzero((f>=first)&(f<=last))
        if len(indices):
            for error in [error_y,error_p]:
                index = indices[np.argmax(error[indices])]
                worst.extend(range(max(0,index-1),min(len(f),index+2)))
    selected = np.unique(np.r_[previous_indices,worst,0,len(f)-1]).astype(int)
    return selected,boost_y,boost_p


class ResidualPressureProblem(VariablePressureProblem):
    def __init__(self,*args,fixed_scale,boost_y,boost_p,**kwargs):
        super().__init__(*args,**kwargs)
        if (fixed_scale.shape!=(self.target_p.shape[1],self.delays.shape[1]*len(self.omega)) or
                np.any(fixed_scale<=0) or not np.isfinite(fixed_scale).all()):
            raise ValueError('positive fixed previous-coordinate readout scale required')
        self.scale = fixed_scale.copy()
        f = self.frequency
        df = np.r_[(f[1]-f[0])/2,(f[2:]-f[:-2])/2,(f[-1]-f[-2])/2]
        for boost in [boost_y,boost_p]:
            if boost.shape!=f.shape or not np.isfinite(boost).all() or np.any(boost<=0):
                raise ValueError('positive fixed frequency importance required')
        self.wy *= boost_y/float(df @ boost_y/df.sum())
        self.wp *= boost_p/float(df @ boost_p/df.sum())

    def prepare(self,x):
        # fit_variable calls preparation; resuming must keep the original
        # physical coefficient penalty instead of rescaling at the warm start.
        if self.scale is None:
            raise ValueError('previous fixed scale missing')
