import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import mido
import numpy as np
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_concert_shared_kernels as kernels
from concert_grand_performance_plan import plan_midi, summarize
from concert_grand_performance import play, PerformanceControl
from concert_grand_integrated_phrase import trajectory
from concert_grand_shared_strings import SharedHammers, SharedDampers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_packed_air import PackedResidues, PackedContact, PackedCoast
from concert_grand_register import make_register
import importlib.util


class PerformanceTests(unittest.TestCase):
    def fixture(self):
        core, full, felt = kernels.KernelTests().fixture()
        air = project_air_preserving_relaxation(full, np.eye(3)[:, :2])
        system = SharedHammers(BatchedStrings(core), felt)
        residues = PackedResidues(air.wave)
        return system, PackedContact(system, air, residues), PackedCoast(system, air, residues), SharedDampers(system.string, system.nh)

    def test_tempo_tracks_zero_velocity_and_controller_values_survive_planning(self):
        midi = mido.MidiFile(type=1, ticks_per_beat=960)
        tempo = mido.MidiTrack([
            mido.MetaMessage('set_tempo', tempo=500000),
            mido.MetaMessage('set_tempo', tempo=750000, time=960),
            mido.MetaMessage('end_of_track', time=960)])
        notes = mido.MidiTrack([
            mido.Message('note_on', note=60, velocity=84, time=1),
            mido.Message('control_change', control=64, value=63, time=479),
            mido.Message('control_change', control=67, value=93, time=480),
            mido.Message('note_on', note=60, velocity=0, time=480),
            mido.Message('note_on', note=65, velocity=124, time=1),
            mido.Message('note_off', note=65, time=479)])
        midi.tracks.extend([tempo, notes])
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'tempos.mid'
            midi.save(path)
            plan = plan_midi(path)
        self.assertEqual(plan['duration_s'], 1.25)
        self.assertEqual([e['kind'] for e in plan['events']], ['on', 'cc', 'cc', 'off', 'on', 'off'])
        self.assertEqual(plan['events'][3]['tick'], 224000)  # .5 + .375 s
        self.assertEqual([e['value'] for e in plan['events'][1:3]], [63, 93])
        self.assertLessEqual(plan['maximum_quantization_error_s'], .5 / 256000)
        self.assertEqual(plan['unsupported_for_faithful_render'], ['continuous_sustain', 'soft_pedal'])
        self.assertEqual(summarize(plan)['missing_current_keys'], [65])

    def test_bounded_chunks_preserve_frozen_contact_restrike_pedal_and_release(self):
        system, contact, coast, dampers = self.fixture()
        # Odd event tick forces exact event handling between coast ticks.
        schedule = [(0., 'on', 60, 84), (.002, 'off', 60, 0),
                    (.0030078125, 'on', 61, 90), (.006, 'off', 61, 0),
                    (.01, 'on', 60, 84), (.014, 'off', 60, 0),
                    (.016, 'pedal_off', 0, 0)]
        reference, trace, metrics = trajectory(system, contact, coast, dampers, True,
                                               duration=.02, contact_rate=256000,
                                               coast_rate=128000, schedule=schedule)
        events = [dict(tick=0, kind='cc', key=64, value=127, channel=0)]
        events.extend(dict(tick=round(t * 256000), kind='cc' if kind == 'pedal_off' else kind,
                           key=64 if kind == 'pedal_off' else key, value=value, channel=0)
                      for t, kind, key, value in schedule)
        pieces = []
        def collect(start, chunk):
            t, b, v = chunk
            take = slice(None) if not pieces else slice(1, None)
            pieces.append((start + t[take], b[take], v[take]))
        end, result = play(system, contact, coast, dampers, events, 5120, collect, block_s=.004)
        for a, b in zip(end, reference):
            np.testing.assert_array_equal(a, b)
        joined = tuple(np.concatenate([piece[i] for piece in pieces]) for i in range(3))
        for a, b in zip(joined, trace):
            np.testing.assert_allclose(a, b, rtol=1e-11, atol=1e-14)
        self.assertEqual(result['contact_steps'], metrics['contact_steps'])
        self.assertEqual(result['coast_steps'], metrics['coast_steps'])
        self.assertEqual(result['damper_heat_j'], metrics['damper_heat_j'])
        self.assertEqual(result['chunks'], 5)
        self.assertLessEqual(result['maximum_buffered_knots'], 513)

    def test_explicit_mechanism_guards_and_overlap_damper_history(self):
        control = PerformanceControl([60])
        pedal = dict(tick=0, kind='cc', key=64, value=90, channel=0)
        with self.assertRaisesRegex(ValueError, 'continuous sustain'):
            control.validate([pedal])
        with self.assertRaisesRegex(ValueError, 'soft pedal'):
            control.validate([dict(pedal, key=67)])
        with self.assertRaisesRegex(ValueError, 'constructed physical bank'):
            control.validate([dict(pedal, kind='on', key=65)])
        on = dict(tick=0, kind='on', key=60, value=84, channel=0)
        control.event(on, 0.)
        control.event(on, .1)
        control.event(dict(on, kind='off'), .2)
        self.assertEqual(control.levels(.25)[60], 0.)
        control.event(dict(on, kind='off'), .3)
        self.assertEqual(control.levels(.34)[60], 1.)
        control.event(dict(pedal, value=127), .35)
        self.assertEqual(control.levels(.36)[60], 0.)
        control.event(dict(pedal, value=0), .4)
        self.assertEqual(control.levels(.4)[60], 0.)
        self.assertEqual(control.levels(.44)[60], 1.)

    def test_equal_rates_still_use_coast_for_separated_heads(self):
        system, contact, coast, dampers = self.fixture()
        end, metrics = play(system, contact, coast, dampers, [], 256, lambda *_: None,
                            contact_rate=128000, coast_rate=128000, block_s=.001)
        self.assertEqual(metrics['contact_steps'], 0)
        self.assertEqual(metrics['coast_steps'], 256)
        self.assertEqual(metrics['chunks'], 2)
        self.assertEqual(system.energy(*end[:2]), 0.)

    def test_register_extends_keys_and_reproduces_frozen_three_key_construction(self):
        modal = SimpleNamespace(frequency=np.array([110., 270., 680.]),
                                damping=np.diag([.01, .02, .03]))
        geometry = dict(panel=dict(length=1.6, width=1.2), nx=2, ny=2,
                        boundary='simply-supported', vectors=np.eye(4)[:, :3])
        path = Path(__file__).resolve().parents[1] / 'render-concert-grand-shared-phrase.py'
        spec = importlib.util.spec_from_file_location('frozen_register', path)
        frozen = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(frozen)
        reference, cents = frozen.make_system(modal, geometry, 8, 3)
        actual, metadata = make_register(modal, geometry, [60, 67, 72], 8, 3)
        self.assertEqual(metadata['wire_tuning_offsets_cents'], cents)
        for name in ['mass', 'K', 'transform', 'damping']:
            np.testing.assert_array_equal(getattr(actual.string, name), getattr(reference.string, name))
        for (_, d, k), (_, rd, rk) in zip(actual.contacts, reference.contacts):
            np.testing.assert_array_equal(d, rd)
            self.assertEqual(k, rk)
        register, metadata = make_register(modal, geometry, [63, 75, 87, 99], 8, 3)
        self.assertEqual(register.keys, [63, 75, 87, 99])
        self.assertEqual(len(register.contacts), 12)
        with self.assertRaisesRegex(ValueError, 'dense core matrix'):
            make_register(modal, geometry, list(range(21, 109)), 96, 3,
                          max_single_matrix_bytes=2**20)


if __name__ == '__main__':
    unittest.main()
