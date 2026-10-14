"""Projection and mechanical diagnostics for an explicitly reduced string model."""
from math import gcd
import numpy as np
from scipy.signal import resample_poly
from scipy.special import roots_legendre
from concert_grand_native_strings import fixture


def modal_indices(source_nt, target_nt):
    if not 1 <= target_nt <= source_nt:raise ValueError('projection must retain a positive modal prefix')
    source_nl=2*source_nt;target_nl=2*target_nt;source_width=4*source_nt
    return np.concatenate([np.r_[np.arange(i*source_width,i*source_width+target_nl),
        np.arange(i*source_width+source_nl,i*source_width+source_nl+target_nt),
        np.arange(i*source_width+source_nl+source_nt,i*source_width+source_nl+source_nt+target_nt)]
        for i in range(3)])


def project(source, nt, rate, duration=None):
    target=fixture(note=source['note'],transverse=nt,rate=rate,seed=None,
                   duration=source['times'][2] if duration is None else duration)
    indices=modal_indices(source['transverse'],nt)
    target['parameters'][:]=source['parameters'];target['parameters'][4]=rate
    target['tensions'][:]=source['tensions'];target['launch_speeds'][:]=0.
    target['shapes']=source['shapes'].reshape(3,source['transverse'])[:,:nt].copy().ravel()
    for key in ['q','v','endpoint']:target[key]=np.r_[source[key][indices],source[key][-1]]
    return target,indices


def omega(f):
    return np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]


def pulse_fixture(peak_force=10.,duration=.096,order=256):
    """Linear response to a prescribed 1 ms half-sine pulse, then free coast.

    This defines the initial state; it does not simulate felt contact.
    The total prescribed force is shared equally between the three wires.
    """
    f=fixture(transverse=96,rate=512000,seed=None,duration=duration)
    f['launch_speeds'][:]=0.;f['q'][:-1]=0.;f['v'][:]=0.
    w=omega(f);nt=f['transverse'];nl=f['axial'];width=nl+2*nt;h=.001
    points,weights=roots_legendre(order);t=h*(points+1)/2
    force=peak_force/3*np.sin(np.pi*t/h)*weights*h/2
    for i in range(3):
        section=slice(i*width+nl,i*width+nl+nt);frequency=w[section]
        shape=-f['shapes'][i*nt:(i+1)*nt]
        phase=frequency[:,None]*(h-t)
        f['q'][section]=shape*((np.sin(phase)/frequency[:,None]) @ force)
        f['v'][section]=shape*(np.cos(phase) @ force)
    f['endpoint']=f['q'].copy()
    return f


def state_errors(source, reference, candidate, indices):
    w=omega(source)
    expected=np.r_[w*np.asarray(reference['q'])[:-1],reference['v'][:-1]]
    q=np.zeros(len(w));v=q.copy();q[indices]=np.asarray(candidate['q'])[:-1];v[indices]=np.asarray(candidate['v'])[:-1]
    actual=np.r_[w*q,v]
    retained=np.r_[indices,len(w)+indices]
    norm=lambda x:float(np.linalg.norm(x))
    return dict(full_state_relative=norm(actual-expected)/max(norm(expected),1e-30),
        retained_state_relative=norm(actual[retained]-expected[retained])/max(norm(expected[retained]),1e-30),
        omitted_linear_energy_fraction=float(np.sum(np.delete(expected**2,retained))/max(np.sum(expected**2),1e-30)))


def common_signal(trace,rate,output_rate=48000):
    data=np.asarray(trace['rows'],dtype=float)
    divisor=gcd(rate,output_rate)
    # A constant physical endpoint is excluded, giving exactly duration*rate samples.
    return resample_poly(data[:-1,1:],output_rate//divisor,rate//divisor,axis=0)


def signal_errors(reference,candidate,output_rate=48000):
    if reference.shape!=candidate.shape:raise ValueError('common observation grid required')
    untrimmed=np.linalg.norm(candidate-reference,axis=0)/np.maximum(np.linalg.norm(reference,axis=0),1e-30)
    # Drop 2 ms at each end to exclude resampling filter boundary transients.
    trim=round(.002*output_rate);a=reference[trim:-trim];b=candidate[trim:-trim]
    relative=np.linalg.norm(b-a,axis=0)/np.maximum(np.linalg.norm(a,axis=0),1e-30)
    window=np.hanning(len(a))[:,None]
    spectrum_a=np.fft.rfft(window*a,axis=0);spectrum_b=np.fft.rfft(window*b,axis=0)
    frequencies=np.fft.rfftfreq(len(a),1/output_rate);bands=[]
    for low,high in [(20,2000),(2000,6000),(6000,12000),(12000,20000)]:
        selected=(frequencies>=low)&(frequencies<high)
        energy=float(np.sum(abs(spectrum_a[selected])**2))
        bands.append(dict(low_hz=low,high_hz=high,
            reference_energy_fraction=energy/max(float(np.sum(abs(spectrum_a)**2)),1e-30),
            complex_spectrum_relative=float(np.linalg.norm(spectrum_b[selected]-spectrum_a[selected])/max(np.sqrt(energy),1e-30))))
    return dict(probe_velocity_relative=relative.tolist(),maximum_probe_velocity_relative=float(max(relative)),
                maximum_probe_velocity_relative_including_boundaries=float(max(untrimmed)),
                bands=bands,output_rate_hz=output_rate,boundary_trim_s=.002,
                acoustic_pressure=False,perceptual_quality_approved=False)
