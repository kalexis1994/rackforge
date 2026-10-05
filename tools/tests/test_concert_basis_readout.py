import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_basis_readout import basis_kernel
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver
from concert_grand_cell_audio import retarded_kernel
import concert_grand_orthotropic_plate as plate


class BasisReadoutTests(unittest.TestCase):
    def test_separable_projection_matches_original_cell_modal_readout(self):
        model=plate.solve(plate.Panel(),5,4,plate.pilot_beams(plate.Panel()),modes=9)
        mics=np.array([[.55,.5,.66],[.85,.5,.66]])
        surface=RectangularLoad.from_plate(model,32,24)
        reference=retarded_kernel(CellObserver(surface,mics,4),128000)
        kernel=basis_kernel(model['panel'],5,4,model['boundary'],[32,24],128000,mics)
        actual=kernel.reshape(len(kernel),2,-1)@model['vectors']
        np.testing.assert_allclose(actual,reference,rtol=0,atol=3e-17)

    def test_nested_coordinates_do_not_depend_on_larger_basis_size(self):
        panel=plate.asdict(plate.Panel());mics=np.array([[.7,.5,.66]])
        a=basis_kernel(panel,3,4,'clamped',[24,20],128000,mics)
        b=basis_kernel(panel,8,9,'clamped',[24,20],128000,mics)
        np.testing.assert_allclose(a,b[:,:,:3,:4],rtol=0,atol=1e-17)

if __name__=='__main__':unittest.main()
