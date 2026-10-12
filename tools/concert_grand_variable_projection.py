"""Positive dynamics with optimal real causal pressure observation at each step.

Readout scaling is frozen at preparation. The ridge penalty is therefore
independent of mechanical parameters when holding readout coefficients fixed.
The envelope gradient includes both physical errors and the readout penalty.
LU factors are shared by forward and microphone-adjoint solves.
"""
import time
import numpy as np
from scipy.linalg import lu_factor,lu_solve,solve
from scipy.optimize import minimize
from concert_grand_storage_preserving_fit import operators
from concert_grand_joint_pressure_fit import quadrature_weights,readout_transfer


class VariablePressureProblem:
    def __init__(self,omega,ports,base_loss,basis,frequency,target_y,target_p,delays,
                 rank=8,readout_ridge=1e-6,parameter_ridge=1e-7):
        self.omega,self.ports,self.base_loss,self.basis = omega,ports,base_loss,basis
        self.frequency,self.target_y,self.target_p,self.delays = frequency,target_y,target_p,delays
        self.rank,self.readout_ridge,self.parameter_ridge = rank,readout_ridge,parameter_ridge
        self.s = 2j*np.pi*frequency
        self.wy,self.wp = quadrature_weights(frequency,target_y),quadrature_weights(frequency,target_p)
        self.phase = np.exp(-self.s[:,None,None]*delays[None])
        self.scale = None

    def states(self,k,c):
        inverse = 1/self.omega
        kk,cc = k*inverse[:,None]*inverse[None],c*inverse[:,None]*inverse[None]
        inertia = np.diag(inverse**2)
        rhs = inverse[:,None]*self.ports
        factors,q = [],[]
        for s in self.s:
            factor = lu_factor(kk+s*cc+s*s*inertia,check_finite=False)
            factors.append(factor)
            q.append(inverse[:,None]*lu_solve(factor,rhs,check_finite=False))
        return np.asarray(q),factors

    def design(self,q,microphone):
        acceleration = self.s[:,None,None]**2*q
        design = np.einsum('fg,fnp->fpgn',self.phase[:,microphone],acceleration).reshape(
            len(self.frequency)*self.ports.shape[1],-1)
        return design*np.repeat(np.sqrt(self.wp),self.ports.shape[1])[:,None]

    def prepare(self,x):
        k,c,_,_ = operators(x,self.omega,self.base_loss,self.basis,self.rank)
        q,_ = self.states(k,c)
        self.scale = np.array([np.maximum(np.linalg.norm(self.design(q,m),axis=0),1e-30)
                              for m in range(self.target_p.shape[1])])

    def optimal_readout(self,q):
        if self.scale is None:
            raise ValueError('fixed initial column scales required')
        readouts,penalty,stationarity = [],0.,0.
        for m in range(self.target_p.shape[1]):
            design = self.design(q,m)/self.scale[m]
            target = (self.target_p[:,m]*np.sqrt(self.wp)[:,None]).reshape(-1)
            gram = (design.conj().T @ design).real
            rhs = (design.conj().T @ target).real
            gram += self.readout_ridge*np.eye(len(gram))
            beta = solve(gram,rhs,assume_a='pos',check_finite=False)
            stationarity = max(stationarity,float(np.linalg.norm(gram @ beta-rhs)/max(np.linalg.norm(rhs),1e-30)))
            penalty += .5*self.readout_ridge*float(beta @ beta)
            readouts.append((beta/self.scale[m]).reshape(self.delays.shape[1],len(self.omega)))
        return np.asarray(readouts),penalty,stationarity

    def evaluate(self,x):
        k,c,scaled_loss,factor = operators(x,self.omega,self.base_loss,self.basis,self.rank)
        q,factors = self.states(k,c)
        readout,penalty,stationarity = self.optimal_readout(q)
        observation = readout_transfer(self.frequency,self.delays,readout)
        inverse = 1/self.omega
        t = np.array([inverse[:,None]*lu_solve(f, inverse[:,None]*o.T,check_finite=False)
                      for f,o in zip(factors,observation)])
        y = self.s[:,None,None]*np.einsum('na,fnb->fab',self.ports,q,optimize=True)
        p = self.s[:,None,None]**2*np.einsum('fmn,fnb->fmb',observation,q,optimize=True)
        ey,ep = y-self.target_y,p-self.target_p
        ly = .5*float(np.sum(self.wy*np.linalg.norm(ey,axis=(1,2))**2))
        lp = .5*float(np.sum(self.wp*np.linalg.norm(ep,axis=(1,2))**2))
        ry = np.einsum('fna,fab->fnb',q,ey.conj(),optimize=True)
        rp = np.einsum('fna,fab->fnb',t,ep.conj(),optimize=True)
        scaled = self.omega[:,None]*self.basis
        pq = np.einsum('fnp,nr->fpr',q,scaled,optimize=True)
        py = np.einsum('fnp,nr->fpr',ry,scaled,optimize=True)
        pp = np.einsum('fnp,nr->fpr',rp,scaled,optimize=True)
        gk = (np.einsum('f,fpr,fpr->r',-self.s*self.wy,py,pq,optimize=True)
            + np.einsum('f,fpr,fpr->r',-self.s**2*self.wp,pp,pq,optimize=True)).real
        cq = np.einsum('nm,fmp->fnp',scaled_loss,q,optimize=True)
        crp = np.einsum('nm,fmp->fnp',scaled_loss,rp,optimize=True)
        gd = (np.einsum('f,fnp,fnp->n',-2*self.s**2*self.wy,ry,cq,optimize=True)
            + np.einsum('f,fnp,fnp->n',-self.s**3*self.wp,rp,cq,optimize=True)
            + np.einsum('f,fnp,fnp->n',-self.s**3*self.wp,crp,q,optimize=True)).real
        ql = np.einsum('fnp,nr->fpr',q,factor,optimize=True)
        rpl = np.einsum('fnp,nr->fpr',rp,factor,optimize=True)
        gl = (np.einsum('f,fnp,fpr->nr',-2*self.s**2*self.wy,ry,ql,optimize=True)
            + np.einsum('f,fnp,fpr->nr',-self.s**3*self.wp,rp,ql,optimize=True)
            + np.einsum('f,fnp,fpr->nr',-self.s**3*self.wp,q,rpl,optimize=True)).real
        gradient = np.r_[-np.exp(-x[:self.basis.shape[1]])*gk,gd,
            (np.sqrt(np.diag(self.base_loss))[:,None]*gl).ravel()]+self.parameter_ridge*x
        value = ly+lp+penalty+.5*self.parameter_ridge*float(x @ x)
        return value,gradient,dict(mobility_objective=ly,pressure_objective=lp,
            readout_penalty=penalty,inner_stationarity_relative=stationarity),readout


def fit_variable(problem,initial,max_iterations=300,checkpoint=None,progress=None):
    problem.prepare(initial)
    started = last = time.perf_counter()
    iteration = 0
    history = []
    def callback(x):
        nonlocal last,iteration
        iteration += 1
        if time.perf_counter()-last>=25:
            value,gradient,parts,readout = problem.evaluate(x)
            row = dict(seconds=time.perf_counter()-started,iteration=iteration,objective=value,
                gradient_max=float(abs(gradient).max()),**parts)
            history.append(row)
            if checkpoint:
                checkpoint(x,problem.basis,problem.scale,readout,row)
            if progress:
                progress(row)
            last = time.perf_counter()
    def objective(x):
        value,gradient,_,_ = problem.evaluate(x)
        return value,gradient
    n,r = len(problem.omega),problem.basis.shape[1]
    result = minimize(objective,initial,jac=True,method='L-BFGS-B',callback=callback,
        bounds=[(-3.,3.)]*r+[(-2.5,2.5)]*n+[(None,None)]*(n*problem.rank),
        options=dict(maxiter=max_iterations,ftol=1e-10,gtol=1e-7,maxls=40,maxcor=30))
    value,gradient,parts,readout = problem.evaluate(result.x)
    if checkpoint:
        checkpoint(result.x,problem.basis,problem.scale,readout,dict(iteration=int(result.nit),objective=value,**parts))
    k,c,_,_ = operators(result.x,problem.omega,problem.base_loss,problem.basis,problem.rank)
    return k,c,readout,dict(optimizer_success=bool(result.success),optimizer_message=str(result.message),
        optimizer_iterations=int(result.nit),objective=value,gradient_max=float(abs(gradient).max()),**parts,
        readout_ridge=problem.readout_ridge,parameter_ridge=problem.parameter_ridge,
        fixed_initial_readout_scaling=True,optimal_readout_each_evaluation=True,
        fit_frequency_count=len(problem.frequency),fit_frequency_hz=problem.frequency.tolist(),progress=history,
        negative_poles_or_loss_repair=False)
