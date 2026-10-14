import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_modal_viability import omega
from concert_grand_hybrid_midi import light_fixture,light_omega,bridge_force


class LightInputTests(unittest.TestCase):
    def test_serialized_inputs_and_frequencies_match_dense_constructor(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder=Path(tmp)
            for note,nt in [(44,12),(63,96),(99,96)]:
                old=fixture(note=note,transverse=nt,rate=64000,duration=.137);old['launch_speeds'][:]=0.
                new=light_fixture(note,64000,.137,nt)
                write_fixture(folder/f'old-{note}',old);write_fixture(folder/f'new-{note}',new)
                self.assertEqual((folder/f'old-{note}').read_bytes(),(folder/f'new-{note}').read_bytes())
                np.testing.assert_allclose(light_omega(new),omega(old),rtol=2e-15,atol=1e-12)


class QuietVoiceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.folder=Path(cls.temp.name);cls.exe=cls.folder/'strings.exe'
        root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O','--cfg','concert_phase_library',
            str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(cls.exe)],check=True)

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def render(self,sleep):
        folder=self.folder/('sleep' if sleep else 'plain');folder.mkdir()
        f=light_fixture(63,32000,1.65,12);write_fixture(folder/'input',f)
        events='0 64 127\n0 67 127\n0 0 28\n.01 1 0\n.02 64 0\n1.3 64 127\n1.5 0 28\n1.55 1 0\n1.56 64 0\n'
        (folder/'events').write_text(events,encoding='utf-8');np.full(12,.4,dtype='<f8').tofile(folder/'loss')
        command=[str(self.exe),str(folder/'input'),'midi-voice','8','16',str(folder/'events'),str(folder/'coast'),str(folder/'loss')]
        if sleep:command.append('--sleep-j=1e-18')
        data=json.loads(subprocess.check_output(command,text=True))
        force=bridge_force(np.fromfile(folder/'coast',dtype='<f8'),data['contact_segments'],
            data['contact_rate_hz'],data['capture_rate_hz'])
        return data,force

    def test_sleep_preserves_axial_carriers_and_wakes_on_live_midi_events(self):
        a,fa=self.render(False);b,fb=self.render(True)
        self.assertEqual(b['attacks'],2);self.assertGreater(b['sleep_episodes'],0)
        self.assertGreater(b['skipped_steps'],1000)
        self.assertLessEqual(b['discarded_transverse_energy_j'],b['sleep_episodes']*1e-18)
        self.assertLess(np.linalg.norm(fb-fa)/np.linalg.norm(fa),1e-5)
        self.assertTrue(np.isfinite(np.r_[b['q'],b['v']]).all())
        np.testing.assert_allclose(b['launch_work_j'],a['launch_work_j'],rtol=1e-6,atol=1e-14)
        # Axial waves are rotated during sleep rather than zeroed on wake.
        axial=np.concatenate([np.arange(i*48,i*48+24) for i in range(3)])
        np.testing.assert_allclose(np.asarray(b['q'])[axial],np.asarray(a['q'])[axial],rtol=1e-3,atol=1e-13)


if __name__=='__main__':unittest.main()
