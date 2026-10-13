"""Causal sampled pressure on the reciprocal loaded body, with fixed FIR latency.

Exact held-force interval accelerations are observed, never state-reset or
filtered per mode. Only eight scalar microphone groups have delay histories.
Continuous harmonic fitting and sampled pressure accuracy remain separate.
"""
from pathlib import Path
import struct
import numpy as np
from concert_grand_realtime_body import Body
from concert_grand_modal_runtime import ModalRuntime
from concert_grand_reciprocal_runtime import ReciprocalRuntime


class PressureRuntime:
    def __init__(self, omega, damping, ports, mapping, delays, readout, rate):
        self.canonical = ReciprocalRuntime(ModalRuntime.from_body(Body.mechanical(omega, damping, ports)))
        self.modal = self.canonical.modal
        self.step = self.modal.coefficients(rate)
        self.mapping = np.asarray(mapping)
        self.delays = np.asarray(delays)
        self.groups = self.delays.size
        n = len(omega)
        # The damped states reconstruct x=(Omega*q,v). Observing delta-v/dt
        # gives the exact interval-mean acceleration, including force jumps.
        self.observation = (readout.reshape(self.groups, n) @ self.modal.vectors[n:]) * self.modal.multiplicity
        self.taps = 16
        self.latency_samples = 7
        fractional = self.delays.ravel()*rate
        self.integer_delay = np.floor(fractional).astype(int)
        phase = fractional-self.integer_delay
        offsets = np.arange(self.taps)
        self.weights = np.sinc(offsets[None]-self.latency_samples-phase[:, None])*np.blackman(self.taps)[None]
        self.weights /= self.weights.sum(axis=1)[:, None]
        self.history_frames = int(self.integer_delay.max()+self.taps+1)
        self.state = np.zeros(len(self.modal.roots), dtype=complex)
        self.history = np.zeros((self.groups, self.history_frames))
        self.cursor = 0
        self._acceleration_output()

    def _acceleration_output(self):
        self.state_observation=self.observation*np.expm1(self.modal.roots*self.step.dt)[None]/self.step.dt
        self.force_observation=(self.observation @ (self.step.drive_step[:,None]*self.step.drive)).real/self.step.dt

    def process(self, external_force):
        force = self.mapping.T @ external_force
        u = self.step.drive @ force
        average = self.step.average_state*self.state+self.step.drive_average*u
        delta = np.expm1(self.modal.roots*self.step.dt)*self.state+self.step.drive_step*u
        velocity = (self.step.output.T @ average).real
        pressure_groups = (self.state_observation @ self.state).real+self.force_observation @ force
        self.state += delta
        self.history[:, self.cursor] = pressure_groups
        result = np.zeros(2)
        for g in range(self.groups):
            indices = (self.cursor-self.integer_delay[g]-np.arange(self.taps)) % self.history_frames
            result[g//self.delays.shape[1]] += self.weights[g] @ self.history[g, indices]
        self.cursor = (self.cursor+1) % self.history_frames
        return self.mapping @ velocity, result

    def harmonic_response(self, frequency):
        """Discrete held-force/mean-acceleration response, including declared FIR latency."""
        frequency = np.atleast_1d(frequency)
        z = np.exp(2j*np.pi*frequency*self.step.dt)
        transition = self.step.transition
        drive = self.step.drive_step[:, None]*self.step.drive
        response = []
        paired = self.modal.multiplicity == 2
        for zz in z:
            residue = self.state_observation/self.modal.multiplicity
            result = residue @ (drive/(zz-transition[:,None]))
            result += residue[:, paired].conj() @ (
                drive[paired].conj()/(zz-transition[paired,None].conj()))
            result += self.force_observation
            fir = np.sum(self.weights*zz**(-(self.integer_delay[:, None]+np.arange(self.taps))), axis=1)
            result *= fir[:, None]
            response.append(result.reshape(2, self.delays.shape[1], -1).sum(axis=1) @ self.mapping.T)
        return np.asarray(response)

    def acceleration_response(self, frequency):
        """Actual discrete interval accelerations in mechanical modal coordinates."""
        transition = self.step.transition
        increment = np.expm1(self.modal.roots*self.step.dt)
        drive = self.step.drive_step[:, None]*self.step.drive
        velocity = self.modal.vectors[len(self.modal.vectors)//2:]
        paired = self.modal.multiplicity == 2
        result = []
        for hz in frequency:
            z = np.exp(2j*np.pi*hz*self.step.dt)
            delta = increment[:, None]*drive/(z-transition[:, None])+drive
            a = velocity @ delta
            a += velocity[:, paired].conj() @ (
                increment[paired, None].conj()*drive[paired].conj()/(z-transition[paired, None].conj())+drive[paired].conj())
            result.append(a/self.step.dt)
        return np.asarray(result)

    def mobility_response(self, frequency):
        paired=self.modal.multiplicity==2
        drive=self.step.drive_step[:,None]*self.step.drive
        result=[]
        for hz in frequency:
            z=np.exp(2j*np.pi*hz*self.step.dt)
            mean=self.step.average_state[:,None]*drive/(z-self.step.transition[:,None])+self.step.drive_average[:,None]*self.step.drive
            y=self.modal.output.T @ mean
            y+=self.modal.output[paired].conj().T @ (
                self.step.average_state[paired,None].conj()*drive[paired].conj()/(z-self.step.transition[paired,None].conj())
                +self.step.drive_average[paired,None].conj()*self.step.drive[paired].conj())
            result.append(y)
        return np.asarray(result)

    def delay_response(self, frequency):
        z = np.exp(2j*np.pi*np.asarray(frequency)*self.step.dt)
        return np.sum(self.weights[None]*z[:,None,None]**(-(self.integer_delay[None,:,None]+np.arange(self.taps))),axis=2).reshape(len(z),*self.delays.shape)

    def set_readout(self, readout):
        n = len(self.modal.vectors)//2
        readout = np.asarray(readout)
        if readout.shape != (*self.delays.shape,n) or not np.isfinite(readout).all():
            raise ValueError('finite real microphone readout with unchanged groups')
        self.observation = (readout.reshape(self.groups,n) @ self.modal.vectors[n:])*self.modal.multiplicity
        self._acceleration_output()

    def set_state_readout(self, state_readout, force_readout):
        state_readout=np.asarray(state_readout)
        force_readout=np.asarray(force_readout)
        if (state_readout.shape!=(*self.delays.shape,len(self.modal.vectors))
                or force_readout.shape!=(*self.delays.shape,self.mapping.shape[1])
                or not np.isfinite(state_readout).all() or not np.isfinite(force_readout).all()):
            raise ValueError('matching real state and force readouts required')
        self.state_observation=(state_readout.reshape(self.groups,-1) @ self.modal.vectors)*self.modal.multiplicity
        self.force_observation=force_readout.reshape(self.groups,-1)

    def write(self, path):
        step = self.step
        values = np.column_stack([part for value in [step.transition, step.average_state,
            step.drive_step, step.drive_average, np.expm1(self.modal.roots*step.dt)] for part in [value.real, value.imag]])
        with Path(path).open('xb') as stream:
            stream.write(struct.pack('<8sIIIIId', b'RFPRE002', len(self.modal.roots),
                self.mapping.shape[1], self.mapping.shape[0], self.groups, self.history_frames, step.dt))
            for a in [values, step.drive.real, step.drive.imag, self.modal.multiplicity,
                      self.mapping, self.state_observation.real, self.state_observation.imag,self.force_observation]:
                stream.write(np.asarray(a, dtype='<f8').tobytes())
            stream.write(np.asarray(self.integer_delay, dtype='<u4').tobytes())
            stream.write(np.asarray(self.weights, dtype='<f8').tobytes())


def fit_digital_readout(runtime, frequency, target, ridge=1e-6):
    """Training-only real readout for the exact consumer and its causal FIR.

    K/C/ports are unchanged. The shared FIR latency is declared and retained;
    it is removed for comparison only, never by looking ahead in the callback.
    """
    from scipy.linalg import solve
    f = np.asarray(frequency)
    acc = runtime.acceleration_response(f)
    phase = runtime.delay_response(f)
    target = target*np.exp(-2j*np.pi*f[:,None,None]*runtime.latency_samples*runtime.step.dt)
    df = np.r_[(f[1]-f[0])/2,(f[2:]-f[:-2])/2,(f[-1]-f[-2])/2]
    weight = np.sqrt(df/df.sum())/np.maximum(np.linalg.norm(target,axis=(1,2)),1e-30)
    readout=[]
    for m in range(2):
        design=np.einsum('fg,fnp->fpgn',phase[:,m],acc).reshape(len(f)*acc.shape[2],-1)
        design*=np.repeat(weight,acc.shape[2])[:,None]
        rhs=(target[:,m]*weight[:,None]).reshape(-1)
        scale=np.maximum(np.linalg.norm(design,axis=0),1e-30)
        design/=scale
        gram=(design.conj().T @ design).real
        beta=solve(gram+ridge*np.eye(len(gram)),(design.conj().T @ rhs).real,assume_a='pos')
        readout.append((beta/scale).reshape(runtime.delays.shape[1],acc.shape[1]))
    return np.asarray(readout)


def fit_digital_state_readout(runtime, frequency, target, ridge=1e-8):
    """General real C/D observation; same body states, ports, and FIR delays.

    Fits state and instantaneous held-force terms jointly, instead of imposing
    a continuous acceleration derivative on a sampled/held-force trajectory.
    Pressure stays outside the mechanical feedback loop.
    """
    from scipy.linalg import solve,null_space
    f=np.asarray(frequency)
    drive=runtime.step.drive_step[:,None]*runtime.step.drive
    paired=runtime.modal.multiplicity==2
    states=[]
    for hz in f:
        z=np.exp(2j*np.pi*hz*runtime.step.dt)
        x=runtime.modal.vectors @ (drive/(z-runtime.step.transition[:,None]))
        x+=runtime.modal.vectors[:,paired].conj() @ (drive[paired].conj()/(z-runtime.step.transition[paired,None].conj()))
        states.append(x)
    # A static force cannot radiate steady pressure. Its first-order response
    # must vanish too, preserving the acceleration-like double zero at DC.
    # Enforce both identities algebraically, instead of fitting tiny numbers.
    static=runtime.modal.vectors @ (drive/(1-runtime.step.transition[:,None]))
    static+=runtime.modal.vectors[:,paired].conj() @ (drive[paired].conj()/(1-runtime.step.transition[paired,None].conj()))
    first=-runtime.step.dt*(runtime.modal.vectors @ (drive/(1-runtime.step.transition[:,None])**2)
        +runtime.modal.vectors[:,paired].conj() @ (drive[paired].conj()/(1-runtime.step.transition[paired,None].conj())**2))
    static=static.real;first=first.real
    basis=null_space((first/np.maximum(np.linalg.norm(first,axis=0),1e-30)).T)
    features=np.einsum('nr,fnp->frp',basis,np.asarray(states)-static,optimize=True)
    phase=runtime.delay_response(f)
    target=target*np.exp(-2j*np.pi*f[:,None,None]*runtime.latency_samples*runtime.step.dt)
    df=np.r_[(f[1]-f[0])/2,(f[2:]-f[:-2])/2,(f[-1]-f[-2])/2]
    weight=np.sqrt(df/df.sum())/np.maximum(np.linalg.norm(target,axis=(1,2)),1e-30)
    readout=[]
    for m in range(2):
        design=np.einsum('fg,fnp->fpgn',phase[:,m],features).reshape(len(f)*features.shape[2],-1)
        design*=np.repeat(weight,features.shape[2])[:,None]
        rhs=(target[:,m]*weight[:,None]).reshape(-1)
        scale=np.maximum(np.linalg.norm(design,axis=0),1e-30)
        design/=scale
        gram=(design.conj().T @ design).real
        beta=solve(gram+ridge*np.eye(len(gram)),(design.conj().T @ rhs).real,assume_a='pos')
        readout.append((beta/scale).reshape(runtime.delays.shape[1],features.shape[1]))
    state_readout=np.asarray(readout) @ basis.T
    force_readout=-state_readout @ static
    return state_readout,force_readout

