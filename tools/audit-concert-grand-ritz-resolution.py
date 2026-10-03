"""Hold the Ritz modes fixed while auditing spatial source/pressure resolution.

Unit harmonic force, matched source/load, actual frozen native coefficients
and microphone geometry. Numerical convergence is not piano identification.
No audio normalization, coefficient fitting or factory changes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np

import concert_grand_orthotropic_plate as plate


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_model(path):
    with np.load(path) as data:
        return dict(panel=json.loads(str(data['panel'])), boundary=str(data['boundary']),
                    nx=int(data['nx']), ny=int(data['ny']), frequency=data['frequency'],
                    vectors=data['vectors'])


def surface_grid(model, scale, selected):
    nx, ny = 40*scale, 24*scale
    length, width = model['panel']['length'], model['panel']['width']
    x = (np.arange(nx)+.5)/nx*length
    y = (np.arange(ny)+.5)/ny*width
    px = plate.basis(x, length, model['nx'], model['boundary'])[0]
    py = plate.basis(y, width, model['ny'], model['boundary'])[0]
    coefficients = model['vectors'][:, selected].reshape(model['nx'], model['ny'], -1)
    temporary = np.einsum('xm,mnk->xnk', px, coefficients, optimize=True)
    shapes = np.einsum('xnk,yn->xyk', temporary, py, optimize=True)
    xx, yy = np.meshgrid(x, y-.5*width, indexing='ij')
    return xx.ravel(), yy.ravel(), shapes.reshape(nx*ny, -1).T


def source_grid(model, selected, count):
    x = model['panel']['length']*(.1+.8*np.linspace(0, 1, count))
    return plate.shape(model, x, np.full(count, .5*model['panel']['width']))[:, selected]


def interpolate(values, positions):
    along = np.asarray(positions)*(len(values)-1)
    index = np.minimum(along.astype(int), len(values)-2)
    fraction = along-index
    return values[index]*(1-fraction[:, None])+values[index+1]*fraction[:, None]


def observer(shapes, x, y, bank, frequencies, discrete):
    rate, speed = 44100., 343.
    result = []
    pattern = float(bank['pattern'][0])
    common = float(bank['common_path'][0])
    area = 1.4/len(x)
    for side in ('l', 'r'):
        if bank['geometry_shared'][0] == 1:
            px, py, pz, ax, ay, az = [float(bank[f'{k}_{side}'][0])
                                    for k in ('px', 'py', 'pz', 'ax', 'ay', 'az')]
        else:
            half = float(bank['half_spacing'][0])
            px, py, pz = .7+(-half if side == 'l' else half), 0., float(bank['height'][0])
            ax, ay, az = 0., 0., -1.
        dx, dy, dz = x-px, y-py, -pz
        distance = np.sqrt(dx*dx+dy*dy+dz*dz)
        response = (1-pattern)+pattern*(dx*ax+dy*ay+dz*az)/distance
        weights = shapes*(response/distance*area)[None, :]
        if discrete:
            delay = np.maximum(distance-common, 0)*rate/speed
            whole = np.floor(delay)
            fraction = delay-whole
        transfers = []
        # Bound temporary phase storage regardless of requested frequency grid.
        for start in range(0, len(frequencies), 32):
            hz = frequencies[start:start+32]
            omega = 2*np.pi*hz/rate
            if discrete:
                phase = np.exp(-1j*whole[:, None]*omega)
                phase *= (1-fraction[:, None])+fraction[:, None]*np.exp(-1j*omega)
                derivative = rate*(1-np.exp(-1j*omega))
            else:
                phase = np.exp(-2j*np.pi*(distance-common)[:, None]*hz/speed)
                derivative = 2j*np.pi*hz
            transfers.append((weights@phase)*derivative[None, :]*1.2/(2*np.pi))
        result.append(np.concatenate(transfers, axis=1))
    return np.stack(result, axis=2)


def relative(a, b):
    return float(np.linalg.norm(a-b)/max(np.linalg.norm(b), 1e-30))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--expected-sha256', required=True)
    p.add_argument('--structural', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    assert sha(a.binary) == a.expected_sha256
    reference = json.loads((a.structural/'report.json').read_text())
    reports = dict(policy=__doc__, binary_sha256=sha(a.binary),
                   structural_report_sha256=sha(a.structural/'report.json'),
                   script_sha256=sha(Path(__file__)), profiles={})
    positions = np.arange(88)/87
    notes = np.array([42, 48, 60, 72, 84, 90, 96])
    cases = [(int(n), h, float(440*2**((n-69)/12)*h)) for n in notes for h in (1, 2, 3)]
    frequencies = np.unique(np.r_[np.geomspace(20, 8000, 150), [c[2] for c in cases]])
    tuning = a.output/'tuning.txt'
    tuning.write_text('', encoding='utf-8')
    for label, definition in reference['banks'].items():
        asset = a.structural/f'{label}-native.csv'
        assert sha(asset) == definition['native_sha256']
        export = a.output/f'{label}-native-transfer.csv'
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_TUNING=str(tuning.resolve()), CG_RITZ_BOARD=str(asset.resolve()),
                   CG_PHYSICAL_TRANSFER_CSV=str(export.resolve()), RUST_MIN_STACK='33554432')
        with (a.output/f'{label}-export.log').open('w') as log:
            subprocess.run([str(a.binary.resolve()), 'bench::export_physical_transfer',
                            '--exact', '--ignored', '--nocapture', '--test-threads=1'],
                           env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        bank = np.genfromtxt(export, delimiter=',', names=True)
        count = definition['native_modes']
        model = load_model(a.structural/f'{label}.npz')
        selected = np.flatnonzero(model['frequency'] < 800)
        assert len(selected) == count
        assert np.allclose(bank['frequency'][:count], model['frequency'][selected], rtol=1e-6)
        native = np.loadtxt(asset, delimiter=',')
        actual = plate.shape(model, 1.4*(.1+.8*positions), np.full(88, .5))[:, selected]
        sources = {n: interpolate(source_grid(model, selected, n), positions)
                   for n in (16, 32, 64, 128)}
        row = dict(native_transfer_sha256=sha(export),
                   geometry_shared=bool(bank['geometry_shared'][0]),
                   height_m=float(bank['height'][0]), half_spacing_m=float(bank['half_spacing'][0]),
                   pattern=float(bank['pattern'][0]),
                   native_modes=count, source_convergence={}, radiation_convergence={},
                   limits=['low structural bank only; high complement excluded from transfer audit',
                           'unit harmonic force, not a complete piano performance',
                           'no air loading, lid, rim or room identification'])
        for n, shapes in sources.items():
            row['source_convergence'][str(n)] = dict(
                relative_shape_error=relative(shapes, actual),
                maximum_shape_absolute_error=float(abs(shapes-actual).max()))
        assert np.allclose(native[:, 1:17].T, source_grid(model, selected, 16), atol=1e-8)
        # Exact native trapezoidal mechanical response, evaluated independently.
        z = np.exp(-2j*np.pi*frequencies/44100)
        low = bank[:count]
        velocity = (low['drive'][:, None]*(1-z*z)[None, :] /
                    ((1-low['a1'][:, None]*z)*(1-z)-
                     low['a2'][:, None]*low['half_step'][:, None]*z*(1+z)))
        previous = None
        dense = None
        for scale in (1, 2, 4, 8):
            x, y, shapes = surface_grid(model, scale, selected)
            if scale == 1:
                assert np.allclose(native[:, 33:], shapes, atol=1e-8)
                modal_hz = bank['frequency'][:count]
                modal_integral = observer(shapes, x, y, bank, modal_hz, False)
                at_modal = modal_integral[np.arange(count), np.arange(count)]
                # Native two-tap observer contains the absolute path phase;
                # the causal integral removes one common path from every mode.
                at_modal *= np.exp(-2j*np.pi*modal_hz*float(bank['common_path'][0])/343)[:, None]
                z_modal = np.exp(-2j*np.pi*modal_hz/44100)
                native_at_modal = np.column_stack((low['out_l']+low['out_l1']*z_modal,
                                                   low['out_r']+low['out_r1']*z_modal))
                row['native_observer_at_modal_relative_error'] = relative(at_modal, native_at_modal)
                assert row['native_observer_at_modal_relative_error'] < .001
            transfer = observer(shapes, x, y, bank, frequencies, True)
            pressure = np.einsum('nm,mf,mfs->nfs', actual, velocity, transfer, optimize=True)
            if previous is not None:
                row['radiation_convergence'][str(scale)] = dict(
                    grid=f'{40*scale}x{24*scale}',
                    relative_modal_observer_change=relative(transfer, previous[0]),
                    relative_exact_source_pressure_change=relative(pressure, previous[1]))
            if scale == 1:
                base = (transfer, pressure)
            previous = dense = (transfer, pressure)
            print(label, 'surface', scale, row['radiation_convergence'].get(str(scale)), flush=True)
        row['base_to_dense'] = dict(relative_modal_observer_error=relative(base[0], dense[0]),
                                    relative_exact_source_pressure_error=relative(base[1], dense[1]))
        pressure_by_source = {n: np.einsum('nm,mf,mfs->nfs', s, velocity, dense[0], optimize=True)
                             for n, s in sources.items()}
        exact_pressure = dense[1]
        row['source_pressure_relative_error'] = {str(n): relative(q, exact_pressure)
                                                for n, q in pressure_by_source.items()}
        row['cases'] = []
        for note, harmonic, frequency in cases:
            ni, fi = note-21, int(np.argmin(abs(frequencies-frequency)))
            def pair_db(q):
                return float(10*np.log10(max(np.mean(abs(q[ni, fi])**2), 1e-40)))
            row['cases'].append(dict(note=note, harmonic=harmonic, frequency=frequency,
                coarse_minus_dense_db=pair_db(base[1])-pair_db(exact_pressure),
                source16_minus_exact_db=pair_db(pressure_by_source[16])-pair_db(exact_pressure),
                source64_minus_exact_db=pair_db(pressure_by_source[64])-pair_db(exact_pressure)))
        reports['profiles'][label] = row
        (a.output/'report.json').write_text(json.dumps(reports, indent=2), encoding='utf-8')
    print('Saved resolution controls.', flush=True)


if __name__ == '__main__':
    main()
