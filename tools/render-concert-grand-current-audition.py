"""Audible current 606-board/nonlinear-string pilot with passive reduced air.

Fresh cold attacks, sustained notes and distributed passive note-off. Acoustic
reduction is measured on held-out full-operator responses; no old capsule is
reused as a compatible reduced state. One global listening gain, no EQ/sample
mixing. Large histories only in RAM. Reports in repo, audio only in SHARED.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
import numpy as np
from scipy.linalg import lu_factor, lu_solve
from scipy.interpolate import CubicHermiteSpline
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_wave_cache import digest
from concert_grand_audition_air import project_air_preserving_relaxation, AuditionContact, AuditionCoast
from concert_grand_damped_coast import engagement
from concert_grand_cell_observer import CellObserver, spectral_relative
from concert_grand_cell_audio import retarded_kernel, kernel_transfer, pressure_fft
from concert_grand_seed_resume import authenticated_seed
import concert_grand_orthotropic_plate as plate

spec = importlib.util.spec_from_file_location('current_audition_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)
prior, field = parent.prior, parent.field


def render_source(system, air, speed, duration, off, rate, contact_duration=.008):
    contact = AuditionContact(system, air)
    state = contact.initial_state(speed)
    cr = 768000
    capture = 128000
    B, V = [contact.board_map@state[0]], [contact.board_map@state[1]]
    begun = last = time.perf_counter()
    maximum_force = 0.
    heat = 0.
    saved3 = None
    for i in range(round(cr*contact_duration)):
        state, stats = contact.step(state, 1/cr, True)
        maximum_force = max(maximum_force, stats['force_n'])
        heat += stats['felt_loss_j']
        if i+1 == round(cr*.003):
            saved3 = (state[0].copy(), state[1].copy())
        if (i+1) % (cr//capture) == 0:
            B.append(contact.board_map@state[0]); V.append(contact.board_map@state[1])
        # Stop only after separation, on a common grid for every coast rate.
        # No state is reset and the rest uses the separated-hammer solver.
        if (i+1) % (cr//16000) == 0 and maximum_force > 0 and system.compression(state[0]) < 0:
            contact_duration = (i+1)/cr
            break
        if time.perf_counter()-last > 25:
            print('attack progress', i+1, round(cr*contact_duration), flush=True)
            last = time.perf_counter()
    if system.compression(state[0]) >= 0:
        raise ValueError('hammer still in contact; need longer attack integration')
    attack_seconds = time.perf_counter()-begun
    pilot = AuditionCoast(system, air)
    pilot.oscillator_functions(.5/rate); pilot._air_prepare(1/rate)
    b, v = [B[-1]], [V[-1]]
    begin = last = time.perf_counter()
    heat_damper = heat_material = 0.
    for i in range(round((duration-contact_duration)*rate)):
        now_t = contact_duration+(i+.5)/rate
        if now_t >= off:
            state, stats = pilot.release_step(state, 1/rate, engagement(now_t-off), True)
        else:
            state, stats = pilot.step(state, 1/rate, True)
        heat_damper += stats.get('damper_heat_j', 0.)
        heat_material += stats['material_loss_j']
        b.append(pilot.board_map@state[0]); v.append(pilot.board_map@state[1])
        now = time.perf_counter()
        if now-last > 25:
            print('note progress', round(now_t, 4), '/', duration,
                  'coast seconds', round(now-begin, 1), flush=True)
            last = now
    coast_seconds = time.perf_counter()-begin
    b, v = np.asarray(b), np.asarray(v)
    if rate != capture:
        spline = CubicHermiteSpline(np.arange(len(b))/rate, b, v, axis=0)
        t = np.arange(round((duration-contact_duration)*capture)+1)/capture
        b, v = spline(t), spline(t, 1)
    position = np.vstack([np.asarray(B)[:-1], b])
    velocity = np.vstack([np.asarray(V)[:-1], v])
    metrics = dict(attack_s=attack_seconds, coast_s=coast_seconds, rate_hz=rate,
        contact_duration_s=contact_duration,
        attack_rate_hz=cr, capture_rate_hz=capture, maximum_force_n=maximum_force,
        felt_heat_j=heat, damper_heat_j=heat_damper, board_material_heat_j=heat_material,
        final_mechanical_j=system.energy(state[0], state[1]),
        scope='No complete angular/storage ledger; passive construction and positive measured heat. '
              'Fresh reduced-air cold trajectory, not continuation of full-air capsule.')
    return np.arange(len(position))/capture, position, velocity, metrics, saved3


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--audio-dir', type=Path, required=True)
    p.add_argument('--diagnostic-rank', type=int,
        help='Explicit diagnostic with measured failed accuracy gate; never certify it.')
    p.add_argument('--velocities', type=int, nargs='+', default=[42, 84, 124])
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('preserve previous report')
    a.full_report = prior.DOC/'medidas-P6-aire-bloques/carga128-pade2.json'
    a.board = prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    a.cache = prior.LOCAL/'P6-carga-causal-2026-10-06'
    a.direction_block = 32
    if not (a.cache/'R.npy').is_file():
        raise ValueError('existing unique full R cache required')
    old_path = prior.DOC/'medidas-P6-contacto-banda-superior/contacto-24k.json'
    old = json.loads(old_path.read_text(encoding='utf-8')); prior.verify(old)
    started = time.perf_counter()
    r = dict(stage='construction', all_gates_passed=False, audition_only=True,
        source_sha256={n:digest(Path(__file__).with_name(n)) for n in [
            Path(__file__).name, 'concert_grand_audition_air.py',
            'concert_grand_damped_coast.py', 'concert_grand_polynomial_coast.py',
            'concert_grand_exponential_coast.py', 'concert_grand_cell_audio.py']},
        parent_sha256={str(old_path):digest(old_path)}, board_sha256=digest(a.board),
        cache_manifest_sha256=digest(a.cache/'manifest.json'),
        criteria=dict(held_out_loaded_pressure_relative=.05, temporal_pressure_relative=.05),
        notes=[60, 65, 67, 72], intensities=a.velocities,
        scope='Current 606-coordinate physical board, 96/192 nonlinear single string per pitch; '
              'passive reduced acoustic pilot, assumed wire/felt/velocity map, natural distributed '
              'damper. Not identified Steinway, unison, full-air temporal certificate or plugin.')
    def save():
        r['seconds'] = time.perf_counter()-started
        r['peak_working_set_bytes'] = field.full.reference.peak_memory()
        a.report.parent.mkdir(parents=True, exist_ok=True)
        a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    save()
    approved, modal, Bsource, notes, surface, provider, identity, _ = field.full.prepare_reference(a)
    _, _, full_air = field.full.construct_air(a, approved, provider, surface, identity, 12*2**30, r, save)
    if r['cache_R_sha256'] != old['cache_R_sha256']:
        raise ValueError('changed full acoustic reference')
    mass = full_air.static_mass()
    wave_mass = full_air.wave.static_mass()
    values, vectors = np.linalg.eigh((wave_mass+wave_mass.T)/2)
    print('wave mass eigenvalue range', values[0], values[-1], flush=True)
    root = (vectors*np.sqrt(values))@vectors.T
    inverse = (vectors/np.sqrt(values))@vectors.T
    with np.load(a.board) as d:
        geometry = dict(panel=json.loads(str(d['panel'])), boundary=str(d['boundary']),
            nx=int(d['nx']), ny=int(d['ny']), vectors=d['ritz_vectors']@modal.basis)
    xs = geometry['panel']['length']*(.1+.8*(np.array(r['notes'])-21)/87)
    phi = plate.shape(geometry, xs, np.full(4, geometry['panel']['width']/2)).T
    omega2 = (2*np.pi*modal.frequency)**2
    observer = CellObserver(provider.surface, old['microphones'], 4)
    r['stage'] = 'acoustic response snapshots'; save()
    train = np.geomspace(70., 12000., 48)
    test = np.array([110., 320., 700., 1450., 2700., 4100., 5700., 6900., 9100.])
    frequency = np.unique(np.r_[train, test])
    loads, responses, snapshots = {}, {}, []
    for hz in frequency:
        s = 2j*np.pi*hz
        Z = full_air.impedance(s)
        D = np.diag(omega2+s*s)+s*modal.damping
        V = s*lu_solve(lu_factor(D+s*Z), phi)
        loads[hz], responses[hz] = Z, V
        if hz in train:
            w = root@V
            force = inverse@(Z-full_air.relaxation.impedance(s))@V
            for part in [w.real, w.imag, force.real, force.imag]:
                norms = np.linalg.norm(part, axis=0)
                snapshots.append(part/np.maximum(norms, 1e-30))
    U, singular, _ = np.linalg.svd(np.hstack(snapshots), full_matrices=False)
    H = parent.transfer_real(observer, test)
    r['basis_singular_values'] = singular.tolist()
    r['acoustic_reduction_cases'] = []
    selected = None
    ranks = [a.diagnostic_rank] if a.diagnostic_rank else [24, 40, 64, 96, 128, 192]
    for rank in ranks:
        air = project_air_preserving_relaxation(full_air, U[:, :rank])
        errors = []
        for hz, h in zip(test, H):
            s = 2j*np.pi*hz
            D = np.diag(omega2+s*s)+s*modal.damping
            V = s*lu_solve(lu_factor(D+s*air.impedance(s)), phi)
            reference = responses[hz]
            pressure = s*h@V; expected = s*h@reference
            errors.append(dict(frequency_hz=float(hz),
                pressure_relative=[field.relative(x, y) for x, y in zip(pressure.T, expected.T)],
                board_relative=[field.relative(x, y) for x, y in zip(V.T, reference.T)]))
        worst = max(max(x['pressure_relative']) for x in errors)
        r['acoustic_reduction_cases'].append(dict(rank=rank, held_out=errors,
            worst_pressure_relative=worst,
            static_mass_relative=field.relative(air.static_mass(), mass),
            high_limit_relative=field.relative(air.high_limit(), full_air.high_limit())))
        print('acoustic rank', rank, 'held-out worst pressure', worst, flush=True); save()
        if worst < r['criteria']['held_out_loaded_pressure_relative']:
            selected = air; break
        if a.diagnostic_rank:
            selected = air
            r['accuracy_gate_passed'] = False
            r['diagnostic_reduction_warning'] = (
                'Explicit listening diagnostic despite failed 5% worst pressure gate. '
                'Every measured per-frequency error is preserved; not a full-air certificate.')
    if selected is None:
        raise ValueError('no reduced acoustic candidate within listening response criterion')
    air = selected
    r['selected_acoustic_rank'] = air.wave.inner.modes
    r['reduction_basis_sha256'] = __import__('hashlib').sha256(U[:, :r['selected_acoustic_rank']].tobytes()).hexdigest()
    r['acoustic_wave_states'] = air.wave.states
    r['mechanical_coordinates'] = 606
    print('selected rank', r['selected_acoustic_rank'], flush=True)
    r['stage'] = 'microphone transfer'; save()
    kernel = retarded_kernel(observer, 128000)
    continuous = parent.transfer_real(observer, test)
    approximate = kernel_transfer(kernel, 128000, test)
    r['microphone_kernel_relative'] = [field.relative(x, y) for x, y in zip(approximate, continuous)]
    def system_for(note):
        # Same assumed canonical C4 material, scaled speaking length for pitch.
        # These are prototype conditions, not a fitted keyboard regulation.
        wire = field.Wire(.62*2**((60-note)/12), .0081, 850.)
        support = field.full.ModalSupportMap(modal.frequency, np.eye(606), modal.damping).support(phi[:, r['notes'].index(note)])
        return field.BridgeHammer(field.MovingBridge(wire, support, transverse=96, axial=192),
            field.Felt(.002, 7.5e8, 2.6, .125, .02, .1))
    sys60 = system_for(60)
    r['stage'] = 'short nonlinear temporal audition check'; save()
    pressures = []
    r['temporal_cases'] = []
    rates = [16000, 32000, 64000, 128000]
    for rate in rates:
        t, B, V, metrics, _ = render_source(sys60, air, 5., .04, 1., rate)
        pressure, f, P, size = pressure_fft(t, B, V, kernel)
        pressures.append(P)
        r['temporal_cases'].append(metrics); save()
    errors = [spectral_relative(f, x, pressures[-1], 60., 6000.) for x in pressures]
    r['temporal_pressure_relative_60_6000'] = dict(zip(map(str, rates), errors))
    candidates = [rate for rate, error in zip(rates, errors)
                  if error < r['criteria']['temporal_pressure_relative']]
    r['render_rate_hz'] = min(candidates)
    print('coast rate controls', r['temporal_pressure_relative_60_6000'],
          'selected', r['render_rate_hz'], flush=True)
    del B, V, t, P, pressure, pressures
    a.audio_dir.mkdir(parents=True, exist_ok=True)
    r['audio_files'] = []
    r['render_cases'] = []
    collection = []
    gain = .25
    for note in r['notes']:
        system = system_for(note)
        rendered = []
        for velocity in r['intensities']:
            # Prescribed incident speeds, without rescaling finished audio.
            speed = 5.*(velocity/124)**1.5
            r['stage'] = f'audible note {note}, intensity {velocity}'; save()
            t, B, V, metrics, at3 = render_source(system, air, speed, 1., .35, r['render_rate_hz'])
            pressure, _, _, _ = pressure_fft(t, B, V, kernel)
            pressure = pressure[:len(t)+len(kernel)-1]
            audio = resample_poly(pressure*gain, 3, 8, axis=0).astype(np.float32)
            if not np.isfinite(audio).all() or np.max(abs(audio)) >= .98:
                raise ValueError('fixed gain clips or waveform is nonfinite')
            metrics.update(note=note, intensity=velocity, speed_m_s=speed,
                physical_pressure_peak_pa=float(np.max(abs(pressure))),
                audio_peak=float(np.max(abs(audio))),
                end_rms=float(np.sqrt(np.mean(audio[-2400:]**2))),
                held_rms=float(np.sqrt(np.mean(audio[9600:14400]**2))))
            # The damper acts inside the dynamics; no terminal output fade.
            rendered.append(np.vstack([np.zeros((4800, 2), np.float32), audio,
                                       np.zeros((9600, 2), np.float32)]))
            r['render_cases'].append(metrics); save()
            print('rendered', note, velocity, metrics['audio_peak'], metrics['end_rms'], flush=True)
            del t, B, V, pressure, audio, at3
        group = np.vstack(rendered)
        label = {60:'C4', 65:'F4', 67:'G4', 72:'C5'}[note]
        suffix = 'intensidades' if len(r['intensities']) > 1 else str(r['intensities'][0])
        path = a.audio_dir/f'{label}-{suffix}.wav'
        if path.exists():
            raise ValueError('preserve existing audio')
        wavfile.write(path, 48000, group)
        r['audio_files'].append(dict(path=str(path.resolve()), sha256=digest(path), seconds=len(group)/48000))
        collection.append(group); save()
    path = a.audio_dir/'Notas-modelo-actual.wav'
    if path.exists():
        raise ValueError('preserve existing collection')
    audio = np.vstack(collection)
    wavfile.write(path, 48000, audio)
    r['audio_files'].append(dict(path=str(path.resolve()), sha256=digest(path), seconds=len(audio)/48000))
    r['common_gain_pa_to_full_scale'] = gain
    r['note_off_s'] = .35
    r['release_duration_s'] = .65
    r['source_and_cache_unchanged'] = bool(digest(a.board) == r['board_sha256']
        and digest(a.cache/'R.npy') == r['cache_R_sha256']
        and digest(a.cache/'manifest.json') == r['cache_manifest_sha256'])
    prior.verify(r)
    r['stage'] = 'audible current physical pilot completed'
    save()
    print('AUDIBLE FILES', json.dumps(r['audio_files']), flush=True)


if __name__ == '__main__':
    main()
