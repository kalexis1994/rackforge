"""SI coordinates for one pinned sine-basis string; no output calibration.

Bridge force below is the leading tension contribution, excluding bending
shear. The cycle-average excess-length observer assumes fixed endpoints and
does not integrate axial relaxation or bidirectional energy exchange.
"""
import numpy as np


def mass_normalized(q_m, velocity_over_omega_m, frequencies_hz, modal_mass_kg):
    root=np.sqrt(modal_mass_kg)
    return root*np.asarray(q_m), root*2*np.pi*np.asarray(frequencies_hz)*np.asarray(velocity_over_omega_m)


def linear_energy_j(q_m, velocity_over_omega_m, frequencies_hz, modal_mass_kg):
    omega=2*np.pi*np.asarray(frequencies_hz)
    return .5*modal_mass_kg*np.sum(omega**2*(np.asarray(q_m)**2+np.asarray(velocity_over_omega_m)**2))


def tension_force_projection(length_m,tension_n,harmonics):
    h=np.asarray(harmonics)
    return (-1.0)**h*tension_n*np.pi*h/length_m


def mean_excess_length_fraction(q_m,velocity_over_omega_m,length_m,harmonics):
    k=np.pi*np.asarray(harmonics)/length_m
    # .5/L integral(y_x²)dx; spatial orthogonality L/2 and phase mean a²/2.
    return .125*np.sum(k*k*(np.asarray(q_m)**2+np.asarray(velocity_over_omega_m)**2))


def verify_coordinates():
    from numpy.polynomial.legendre import leggauss
    length,tension,mu=.72,850.,.004
    h=np.arange(1,9);mass=mu*length/2;frequencies=h*np.sqrt(tension/mu)/(2*length)
    q=np.sin(h*.31)*2e-5;o=np.cos(h*.27)*3e-5
    Q,P=mass_normalized(q,o,frequencies,mass)
    modal=.5*np.sum(P*P+(2*np.pi*frequencies*Q)**2)
    x,weight=leggauss(96);x=(x+1)*length/2;weight*=length/2
    shapes=np.sin(x[:,None]*np.pi*h/length)
    slopes=np.cos(x[:,None]*np.pi*h/length)*np.pi*h/length
    speed=shapes@(o*2*np.pi*frequencies)
    spatial=.5*np.sum(weight*(mu*speed*speed+tension*(slopes@q)**2))
    energy_error=abs(modal-spatial)/modal
    amplitudes=np.hypot(q,o);phase=np.linspace(0,2*np.pi,256,endpoint=False)
    positions=amplitudes[:,None]*np.sin(h[:,None]*phase)
    observed=.5/length*np.mean(np.sum(weight[:,None]*(slopes@positions)**2,axis=0))
    strain=mean_excess_length_fraction(q,o,length,h)
    strain_error=abs(observed-strain)/strain
    # Numerical virtual work at the endpoint: force is the conjugate of z.
    g=tension_force_projection(length,tension,h)/np.sqrt(mass)
    z,step=.00001,1e-8
    coupling=lambda zz:-zz*np.dot(g,Q)
    force=-(coupling(z+step)-coupling(z-step))/(2*step)
    virtual_work_error=abs(force-np.dot(g,Q))/max(1e-30,abs(force))
    assert energy_error<1e-12 and strain_error<1e-12 and virtual_work_error<1e-10
    return dict(spatial_energy_relative_error=float(energy_error),
        cycle_and_spatial_strain_relative_error=float(strain_error),
        endpoint_virtual_work_relative_error=float(virtual_work_error))
