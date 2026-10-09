import importlib.util, sys, unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import test_concert_shared_kernels as kernel_tests
from concert_grand_shared_strings import SharedHammers, SharedDampers
from concert_grand_shared_kernels import BatchedStrings, BatchedCoast
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_packed_air import PackedResidues, PackedContact, PackedCoast
from concert_grand_integrated_phrase import trajectory


class IntegrationTests(unittest.TestCase):
    def test_live_restrike_pedal_and_event_between_coarse_ticks(self):
        core,full,felt=kernel_tests.KernelTests().fixture()
        air=project_air_preserving_relaxation(full,np.eye(3)[:,:2])
        system=SharedHammers(BatchedStrings(core),felt)
        residues=PackedResidues(air.wave); dampers=SharedDampers(system.string,system.nh)
        schedule=[(0.,'on',60,84),(.002,'off',60,0),(.0030078125,'on',61,84),
                  (.006,'off',61,0),(.01,'on',60,84),(.014,'off',60,0),(.016,'pedal_off',0,0)]
        a,ta,sa=trajectory(system,ExponentialContact(system,air),BatchedCoast(system,air),dampers,True,
                            duration=.02,schedule=schedule)
        b,tb,sb=trajectory(system,PackedContact(system,air,residues),PackedCoast(system,air,residues),dampers,True,
                            duration=.02,schedule=schedule)
        for x,y in zip(a,b): np.testing.assert_allclose(x,y,rtol=1e-8,atol=3e-17)
        np.testing.assert_array_equal(ta[0],tb[0])
        for x,y in zip(ta[1:],tb[1:]): np.testing.assert_allclose(x,y,rtol=1e-8,atol=3e-17)
        self.assertEqual([x['time_s'] for x in sb['launches']],[0.,.0030078125,.01])
        self.assertGreater(sb['damper_heat_j'],0.)
        self.assertEqual(sb['contact_steps']+8*sb['coast_steps'],2560)
        self.assertLess(abs(sa['felt_heat_j']-sb['felt_heat_j']),1e-14)
        self.assertFalse(sb['complete_energy_certificate'])

    def test_band_measurement_finds_a_brightness_change_at_fixed_gain(self):
        path=Path(__file__).resolve().parents[1]/'render-concert-grand-integrated-phrase.py'
        spec=importlib.util.spec_from_file_location('integration_bands',path)
        module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        t=np.arange(48000)/48000
        low=.1*np.sin(2*np.pi*100*t); high=.05*np.sin(2*np.pi*1000*t)
        reference=np.column_stack([low+high,low+high])
        result=module.compare_audio(reference,reference,48000)
        self.assertEqual(result['waveform_relative'],0.)
        self.assertTrue(result['bands_60_12000_1_percent_passed'])
        candidate=np.column_stack([low+1.1*high,low+1.1*high])
        result=module.compare_audio(candidate,reference,48000)
        self.assertAlmostEqual(result['waveform_relative'],.1/np.sqrt(5),places=12)
        self.assertAlmostEqual(result['bands'][2]['relative_error'],.1,places=12)
        self.assertFalse(result['waveform_1_percent_passed'])
        self.assertFalse(result['bands_60_12000_1_percent_passed'])


if __name__=='__main__': unittest.main()
