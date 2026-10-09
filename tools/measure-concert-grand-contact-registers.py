"""C4/C5 high-velocity contact curves, native separation and solver handoff.

Local numerical audit of unchanged physics. Endpoint brackets are evidence,
not a certificate of missed within-step events or full phrase convergence.
Only small force vectors/report persist; no duplicate R, WAV or warm state.
"""
import argparse, importlib.util, json, time
from pathlib import Path
import numpy as np
from concert_grand_shared_strings import SharedHammers, SharedContact, SharedDampers
from concert_grand_shared_kernels import BatchedStrings, BatchedCoast
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_contact_observer import ObservedContact
from concert_grand_cell_audio import pressure_fft
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('contact_audit', Path(__file__).with_name('measure-concert-grand-shared-kernels.py'))
audit = importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists(): raise ValueError('preserve existing evidence')
    start = time.perf_counter()
    names = [Path(__file__).name, 'concert_grand_contact_observer.py',
             'concert_grand_exponential_contact.py', 'concert_grand_shared_kernels.py',
             'concert_grand_shared_strings.py', 'measure-concert-grand-shared-kernels.py',
             'render-concert-grand-shared-phrase.py', 'concert_grand_audition_air.py']
    report = dict(stage='construction', all_gates_passed=False, approved_for_audition=False,
                  scope='Local 2ms attack and short handoff/release, three keys/nine strings, '
                  '606 board, unchanged frozen rank-192 air. Finite source-interval pressure. '
                  'No full phrase, piano identification or full acoustic certificate.',
                  source_sha256={n:digest(Path(__file__).with_name(n)) for n in names}, cases=[])
    def save():
        report['seconds'] = time.perf_counter()-start
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False), encoding='utf-8')
    save()
    modal, geometry, air, kernel, paths = audit.frozen.prepare(report, save)
    original, _ = audit.frozen.make_system(modal, geometry, 96, 3)
    system = SharedHammers(BatchedStrings(original.string), original.felt)
    contact = SharedContact(system, air); carrier = ExponentialContact(system, air)
    coast = BatchedCoast(system, air); damper = SharedDampers(system.string, system.nh)
    omega = np.sqrt(coast.oscillator_functions(1/128000)[3]); n = system.string.size
    def mechanical(state): return np.r_[omega*coast.project(state[0][:n]), state[1][:n]]
    def execute(label, pilot, initial, duration, rate, key=None, dampers=None):
        report['stage'] = f'{label} {rate}'; save()
        observed = ObservedContact(pilot, key, initial) if key is not None else None
        final, trace, stats = audit.run(observed or pilot, initial, duration, rate, dampers)
        pressure = pressure_fft(*trace, kernel)[0]
        if not all(np.isfinite(x).all() for x in (*final, pressure)):
            raise ValueError('nonfinite trajectory')
        stats.update(label=label, pressure_peak_pa=float(abs(pressure).max()))
        if observed is not None:
            stats.update(observed.observer.summary())
            grid = np.arange(round(duration*128000)+1)/128000
            stats['force_capture_rate_hz'] = 128000
            stats['force_curve_per_wire_n'] = observed.observer.curve(grid).tolist()
        report['cases'].append(stats); save()
        print('CASE', label, rate, stats['seconds'], 'force', stats.get('force_peak_sum_n'),
              'separation', stats.get('first_separation_bracket_s'), flush=True)
        return final, pressure, stats
    def compare(item, reference):
        state, pressure, stats = item; other, reference_pressure, ref = reference
        stats.update(reference_label=ref['label'], reference_rate_hz=ref['rate_hz'],
                     pressure_relative=audit.relative(pressure, reference_pressure),
                     mechanical_state_relative=audit.relative(mechanical(state), mechanical(other)),
                     wave_state_relative=audit.relative(state[2], other[2]),
                     relaxation_state_relative=audit.relative(state[3], other[3]))
        if 'force_curve_per_wire_n' in stats:
            stats['force_curve_relative'] = audit.relative(np.array(stats['force_curve_per_wire_n']),
                                                          np.array(ref['force_curve_per_wire_n']))
            stats['force_impulse_relative'] = audit.relative(np.array(stats['impulse_per_wire_ns']),
                                                            np.array(ref['impulse_per_wire_ns']))
            a, b = stats['first_separation_bracket_s'], ref['first_separation_bracket_s']
            stats['all_first_separations_observed'] = all(x is not None for x in a+b)
            if stats['all_first_separations_observed']:
                stats['separation_midpoint_difference_s'] = [float(abs(np.mean(x)-np.mean(y))) for x,y in zip(a,b)]
                stats['separation_combined_half_bracket_s'] = [(x[1]-x[0]+y[1]-y[0])/2 for x,y in zip(a,b)]
        fields = ['pressure_relative', 'mechanical_state_relative', 'wave_state_relative',
                  'relaxation_state_relative']
        if 'force_curve_relative' in stats: fields += ['force_curve_relative', 'force_impulse_relative']
        stats['local_all_metrics_1_percent_passed'] = all(stats[k] <= .01 for k in fields)
        save(); print('COMPARISON', stats['label'], stats['rate_hz'], {k:stats[k] for k in fields}, flush=True)
    # Strong C5 ref is itself refined; lower/middle cases retain that limitation.
    strongest = None
    for key, velocity in [(72,124), (60,124), (72,84)]:
        initial = contact.initial_state(0.)
        initial, work = system.launch(initial, key, 5*(velocity/124)**1.5)
        prefix = f'key{key}_velocity{velocity}'
        report.setdefault('launch_work_j', {})[prefix] = work; save()
        reference = execute(prefix+'_midpoint', contact, initial, .002, 1536000, key)
        if (key, velocity) == (72,124):
            fine = execute(prefix+'_midpoint', contact, initial, .002, 3072000, key)
            compare(reference, fine); reference = fine
        for rate in [128000,256000] + ([512000] if (key,velocity)==(72,124) else []):
            item = execute(prefix+'_carrier', carrier, initial, .002, rate, key)
            compare(item, reference)
            if (key, velocity, rate) == (72,124,512000): strongest = item[0]
    if strongest is None or system.compression(strongest[0]) >= -1e-12:
        raise ValueError('separated high C5 required for handoff')
    # Same living q/v/air state, no reset at the solver handoff.
    uninterrupted = execute('C5_separated_contact_solver', carrier, strongest, .002, 128000)
    switched = execute('C5_handoff_coast', coast, strongest, .002, 128000)
    compare(switched, uninterrupted)
    report['handoff_arrays_relative'] = [audit.relative(x,y) for x,y in zip(switched[0],uninterrupted[0])]
    report['handoff_equivalence_1e_8_passed'] = max(report['handoff_arrays_relative']) < 1e-8
    # Early lowering only: the entire 30ms ramp/full release remains untested here.
    a = execute('C5_early_damper_lowering', coast, switched[0], .008, 64000, dampers=damper)
    b = execute('C5_early_damper_lowering', coast, switched[0], .008, 128000, dampers=damper)
    compare(a,b)
    report['source_and_board_unchanged'] = all(digest(Path(__file__).with_name(k)) == h for k,h in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256']
    report['stage'] = 'completed contact register and short transition audit'; save()
    print('AUDIT COMPLETED', args.report, flush=True)


if __name__ == '__main__': main()
