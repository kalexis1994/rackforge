"""Compare the frozen board observer with a frequency-dependent Rayleigh integral.

Reads coefficients from bench::export_physical_transfer. The comparison holds
mechanical masses, modes, bridge projections and microphone geometry fixed.
This diagnoses the observer approximation; it does not identify a real piano's
geometry or predict perceived fidelity from steady-state pressure alone.
"""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np

RATE = 44100
LENGTH, WIDTH, SPEED, DENSITY = 1.4, 1., 343., 1.2


def mobility(bank, frequency):
    z = np.exp(-2j*np.pi*frequency/RATE)
    return (bank['drive']*(1-z*z) /
            ((1-bank['a1']*z)*(1-z)-bank['a2']*bank['half_step']*z*(1+z)))


def projection(bank, note):
    # Same interpolation onto the sixteen drive points as Voice::drive_index.
    along = np.clip((note-21)/87, 0, 1)*15
    i = min(int(along), 14)
    fraction = along-i
    def at(x):
        return ((bank['a']+bank['b']*np.cos(np.pi*bank['q']*x)
                 +bank['c']*np.sin(np.pi*bank['q']*x))*np.cos(bank['theta_y']))
    return (1-fraction)*at(i/15)+fraction*at((i+1)/15)


def surface(bank, scale):
    x, y = np.meshgrid((np.arange(40*scale)+.5)/(40*scale),
                       (np.arange(24*scale)+.5)/(24*scale)-.5, indexing='ij')
    x, y = x.ravel(), y.ravel()
    shape = ((bank['a'][:, None]+bank['b'][:, None]*np.cos(np.pi*bank['q'][:, None]*x)
              +bank['c'][:, None]*np.sin(np.pi*bank['q'][:, None]*x))
             *np.cos(np.pi*bank['qy'][:, None]*y+bank['theta_y'][:, None]))
    # The exporter freezes a single geometry for the whole bank.
    for key in ('height', 'half_spacing', 'pattern'):
        assert np.ptp(bank[key]) == 0, f'Geometry varies between modes: {key}'
    height, half, pattern = [float(bank[k][0]) for k in ('height','half_spacing','pattern')]
    distances, weights = [], []
    for capsule in (.5*LENGTH-half, .5*LENGTH+half):
        r = np.sqrt((x*LENGTH-capsule)**2+(y*WIDTH)**2+height**2)
        distances.append(r)
        weights.append(shape*((1-pattern)+pattern*height/r)/r/len(x))
    if 'geometry_shared' in bank.dtype.names and bank['geometry_shared'][0] == 1:
        distances, weights = [], []
        for side in ('l','r'):
            px,py,pz,ax,ay,az = [float(bank[f'{k}_{side}'][0]) for k in ('px','py','pz','ax','ay','az')]
            dx,dy,dz = x*LENGTH-px, y*WIDTH-py, -pz
            r = np.sqrt(dx*dx+dy*dy+dz*dz)
            response = (1-pattern)+pattern*(dx*ax+dy*ay+dz*az)/r
            distances.append(r)
            weights.append(shape*response/r/len(x))
        height = float(bank['common_path'][0])
    return height, distances, weights


def rayleigh(frequency, distances, weights, height=0., discrete=False):
    omega = 2*np.pi*frequency/RATE
    derivative = RATE*(1-np.exp(-1j*omega)) if discrete else 2j*np.pi*frequency
    outputs = []
    for r, w in zip(distances, weights):
        if discrete:
            delay = (r-height)*RATE/SPEED
            whole = np.floor(delay)
            fraction = delay-whole
            phase = np.exp(-1j*omega*whole)*((1-fraction)+fraction*np.exp(-1j*omega))
        else:
            phase = np.exp(-2j*np.pi*frequency*(r-height)/SPEED)
        outputs.append(w@phase*derivative*DENSITY*LENGTH*WIDTH/(2*np.pi))
    return np.stack(outputs, axis=1)


def check_mechanical_recurrence(bank):
    # Independent sinusoidal time stepping checks the exported q/v units and
    # the input+previous-input term in the analytic transfer, off resonance.
    errors = []
    for index in (0, len(bank)//2, len(bank)-1):
        mode = bank[index]
        for frequency in (261.626, 1046.5, 2093.):
            omega = 2*np.pi*frequency/RATE
            transfer = mobility(bank[index:index+1], frequency)[0]
            # Start in the analytic steady state; no arbitrary settling window.
            q_transfer = mode['half_step']*(1+np.exp(-1j*omega))/(1-np.exp(-1j*omega))*transfer
            q = (q_transfer*np.exp(-1j*omega)).real
            v = (transfer*np.exp(-1j*omega)).real
            previous = np.cos(-omega)
            actual, expected = [], []
            for n in range(2048):
                force = np.cos(omega*n)
                next_v = mode['a1']*v+mode['a2']*q+mode['drive']*(force+previous)
                q += mode['half_step']*(v+next_v)
                v, previous = next_v, force
                actual.append(v)
                expected.append((transfer*np.exp(1j*omega*n)).real)
            relative = float(np.linalg.norm(np.array(actual)-expected)/np.linalg.norm(expected))
            assert relative < 1e-8, f'Mechanical transfer mismatch: {relative}'
            errors.append(relative)
    return max(errors)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bank', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--notes', default='42,48,60,72,84,90,96')
    p.add_argument('--base-scale', type=int, choices=(1,2,3,4), default=1)
    p.add_argument('--dense-scale', type=int, choices=(2,3,4,6,8), default=2)
    p.add_argument('--partials', type=int, choices=(3,6), default=3)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    bank = np.genfromtxt(a.bank, delimiter=',', names=True)
    assert a.dense_scale > a.base_scale
    height, distances, weights = surface(bank, a.base_scale)
    _, dense_r, dense_w = surface(bank, a.dense_scale)
    modal = bank['frequency']
    # Reconstruct the current two-tap observer from its exact digital coefficients.
    # A second two-tap observer removes the common height/c at each modal
    # frequency. This is an ablation, not a replacement for the causal integral.
    modal_gain = np.zeros((len(bank), 2), dtype=complex)
    for m, (r, w) in enumerate(zip(distances, weights)):
        modal_gain[:, m] = (w*np.exp(-2j*np.pi*modal[:, None]*(r-height)/SPEED)).sum(axis=1)*1j*DENSITY*modal*LENGTH*WIDTH
    omega_m = 2*np.pi*modal/RATE
    absolute_gain = modal_gain*np.exp(-2j*np.pi*modal[:, None]*height/SPEED)
    native_at_modal = np.column_stack((bank['out_l']+bank['out_l1']*np.exp(-1j*omega_m),
                                      bank['out_r']+bank['out_r1']*np.exp(-1j*omega_m)))
    modal_error = float(np.linalg.norm(native_at_modal-absolute_gain)/np.linalg.norm(absolute_gain))
    assert modal_error < .001, f'Export does not match the native Rayleigh geometry: {modal_error}'
    b = -modal_gain.imag/np.sin(omega_m[:, None])
    c = modal_gain.real-b*np.cos(omega_m[:, None])
    rows = []
    maximum_discrete_error, maximum_grid_error = 0., 0.
    for note in map(int, a.notes.split(',')):
        f0 = 440*2**((note-69)/12)
        coupling = projection(bank, note)
        for partial in range(1,a.partials+1):
            frequency = f0*partial
            z = np.exp(-2j*np.pi*frequency/RATE)
            observers = {
                'current_two_tap': np.column_stack((bank['out_l']+bank['out_l1']*z,
                                                    bank['out_r']+bank['out_r1']*z)),
                'common_path_removed_two_tap': c+b*z,
                'rayleigh_continuous': rayleigh(frequency, distances, weights, height),
                'retarded_discrete': rayleigh(frequency, distances, weights, height, True),
                'rayleigh_dense': rayleigh(frequency, dense_r, dense_w, height),
            }
            v = mobility(bank, frequency)*coupling
            pressure = {key: (v[:, None]*observer).sum(axis=0) for key, observer in observers.items()}
            power = lambda x: float(np.mean(abs(x)**2))
            db = lambda x: float(10*np.log10(max(power(x), 1e-40)))
            exact = pressure['rayleigh_continuous']
            grid_error = 10*np.log10(max(power(pressure['rayleigh_dense']),1e-40)/max(power(exact),1e-40))
            discrete_error = 10*np.log10(max(power(pressure['retarded_discrete']),1e-40)/max(power(exact),1e-40))
            maximum_discrete_error = max(maximum_discrete_error, abs(discrete_error))
            maximum_grid_error = max(maximum_grid_error, abs(grid_error))
            rows.append(dict(note=note, partial=partial, frequency=frequency,
                             pressure_db={key: db(value) for key,value in pressure.items()},
                             dense_minus_base_db=float(grid_error), fir_minus_continuous_db=float(discrete_error)))
    ratios = []
    for note in map(int, a.notes.split(',')):
        h = [row for row in rows if row['note']==note]
        ratios.append(dict(note=note, H2_H1_transfer_db={key:h[1]['pressure_db'][key]-h[0]['pressure_db'][key]
                                                      for key in h[0]['pressure_db']}))
    report = dict(policy=__doc__, sample_rate=RATE, mechanical_modes=len(bank),
                  bank_sha256=hashlib.sha256(a.bank.read_bytes()).hexdigest(),
                  quadrature_scales=[a.base_scale,a.dense_scale],
                  common_path_seconds=height/SPEED, mechanical_recurrence_max_relative_error=check_mechanical_recurrence(bank),
                  geometry_shared=bool('geometry_shared' in bank.dtype.names and bank['geometry_shared'][0] == 1),
                  native_observer_at_modal_relative_error=modal_error,
                  maximum_dense_grid_level_change_db=maximum_grid_error,
                  maximum_discrete_vs_continuous_level_change_db=maximum_discrete_error,
                  rows=rows, ratios=ratios,
                  caveats=['Unit sinusoidal force, no strings, envelopes, rim, auxiliary routes or room.',
                           'Common minimum-path/c removed identically for every mode in the causal observer.',
                           'Steady-state transfer ratios are not measured note attack ratios.',
                           'Positive mechanical masses and drive projections are held fixed.'])
    (a.output/'transfer.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({key:report[key] for key in ('mechanical_modes','mechanical_recurrence_max_relative_error',
                                               'maximum_dense_grid_level_change_db','maximum_discrete_vs_continuous_level_change_db','ratios')}, indent=2))


if __name__ == '__main__':
    main()
