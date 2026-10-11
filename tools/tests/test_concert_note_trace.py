import sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import test_concert_shared_kernels as kernels
from concert_grand_shared_strings import SharedHammers,SharedDampers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_audition_air import project_air_preserving_relaxation
from concert_grand_packed_air import PackedResidues,PackedContact,PackedCoast
from concert_grand_integrated_phrase import trajectory
from concert_grand_note_trace import NoteTrace,release_segment


class NoteTraceTests(unittest.TestCase):
    def test_observed_contact_coast_release_matches_frozen_trajectory(self):
        core,full,felt=kernels.KernelTests().fixture()
        air=project_air_preserving_relaxation(full,np.eye(3)[:,:2])
        system=SharedHammers(BatchedStrings(core),felt);residues=PackedResidues(air.wave)
        contact=PackedContact(system,air,residues);coast=PackedCoast(system,air,residues)
        dampers=SharedDampers(system.string,system.nh)
        args=dict(pedal=False,duration=.02,coast_rate=128000,
                  schedule=[(0.,'on',60,84),(.008,'off',60,0)])
        a,ta,sa=trajectory(system,contact,coast,dampers,contact_rate=256000,**args)
        tracker=NoteTrace(256000,[.008,.012,.02])
        b,tb,sb=tracker.run(system,contact,coast,dampers,**args)
        for x,y in zip(a,b):np.testing.assert_array_equal(x,y)
        for x,y in zip(ta,tb):np.testing.assert_array_equal(x,y)
        for x,y in zip(b,tracker.states[.02]):np.testing.assert_array_equal(x,y)
        self.assertGreater(sb['contact_steps'],0);self.assertGreater(sb['coast_steps'],0)
        self.assertEqual(sb['contact_steps']+2*sb['coast_steps'],5120)
        self.assertEqual(sa['damper_heat_j'],sb['damper_heat_j'])
        # Refine the actual release, after separation, from an observed tuple.
        end,trace,stats=release_segment(coast,dampers,tracker.states[.012],.012,.008,
                                      128000,note_off=.008)
        for x,y in zip(end,b):np.testing.assert_allclose(x,y,rtol=1e-10,atol=1e-15)
        self.assertGreater(stats['damper_heat_j'],0.)
        np.testing.assert_allclose(trace[1],tb[1][1536:],rtol=1e-10,atol=1e-15)


if __name__=='__main__':unittest.main()
