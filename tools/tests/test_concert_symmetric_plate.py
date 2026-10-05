import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
import concert_grand_orthotropic_plate as plate
import concert_grand_symmetric_plate as symmetry

class SymmetricPlateTests(unittest.TestCase):
    def test_exact_operator_block_and_zero_cross_parity(self):
        panel=plate.Panel();beams=plate.pilot_beams(panel)
        M,K=plate.assemble(panel,13,10,beams);m,k,idx=symmetry.assemble(panel,13,10,beams)
        for full,reduced in [(M,m),(K,k)]:
            np.testing.assert_allclose(reduced,full[np.ix_(idx,idx)],rtol=2e-15,atol=0)
            other=np.setdiff1d(np.arange(len(full)),idx)
            self.assertLess(np.linalg.norm(full[np.ix_(idx,other)])/np.linalg.norm(full),1e-14)

    def test_bridge_mobility_and_off_resonance_field_match_full_eigensystem(self):
        panel=plate.Panel();beams=plate.pilot_beams(panel)
        full=plate.solve(panel,13,10,beams);reduced=symmetry.solve(panel,13,10,beams)
        f=np.array([60.,261.625565,523.251131,1200.,3000.,6000.]);points=[(.3,.5),(.642,.5),(1.1,.5)]
        np.testing.assert_allclose(plate.mobility(reduced,f,points),plate.mobility(full,f,points),rtol=2e-10,atol=1e-14)
        field=plate.shape(full,[.2,.55,.8,1.1],[.2,.5,.8,.37]);bridge=plate.shape(full,.642,.5)
        smallfield=plate.shape(reduced,[.2,.55,.8,1.1],[.2,.5,.8,.37]);smallbridge=plate.shape(reduced,.642,.5)
        def response(model,phi,load):
            w=2*np.pi*f[:,None];w0=2*np.pi*model['frequency']
            return (1/(w0*w0-w*w+1j*.046*w*w0)*load)@phi.T
        np.testing.assert_allclose(response(reduced,smallfield,smallbridge),response(full,field,bridge),rtol=2e-10,atol=1e-17)

    def test_asymmetric_bridge_is_rejected(self):
        with self.assertRaises(ValueError):symmetry.solve(plate.Panel(),6,6,[plate.Beam('x',.53,.03,.04,10e9,660)])

if __name__=='__main__':unittest.main()
