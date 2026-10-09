import sys,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_port_half_avf import PortHalfAVFCoast
from concert_grand_port_avf_coast import PortAVFCoast
from concert_grand_coast_controls import quadratic_system
import test_concert_port_avf_coast as parent


class PortHalfAVFTests(parent.PortAVFTests):
    def fixture(self):
        original,state=super().fixture()
        return PortHalfAVFCoast(original.system,original.air,original.packed_residues),state

    def test_quadratic_composition_equals_single_carrier(self):
        pilot,state=self.fixture(); system=quadratic_system(pilot.system)
        single=PortAVFCoast(system,pilot.air,pilot.packed_residues)
        halves=PortHalfAVFCoast(system,pilot.air,pilot.packed_residues)
        for rate in [16000,64000,256000]:
            a,_=single.step(state,1/rate,True)
            b,_=halves.step(state,1/rate,True)
            for x,y in zip(a,b): np.testing.assert_allclose(x,y,rtol=1e-10,atol=1e-14)


if __name__=='__main__': unittest.main()
