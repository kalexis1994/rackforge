import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
import concert_grand_orthotropic_plate as plate
import concert_grand_symmetric_plate as symmetric
from concert_grand_matrixfree_air import RitzSurface,CellAir,MechanicalDynamic,solve_loaded
from concert_grand_real_surface import RealArithmeticSurface,RealMatrix


class RealSurfaceTests(unittest.TestCase):
    def test_real_and_complex_products_keep_map_and_transpose(self):
        rng=np.random.default_rng(423);M=rng.normal(size=(39,13));map=RealMatrix(M)
        for q in [rng.normal(size=13),rng.normal(size=(13,7))+1j*rng.normal(size=(13,7))]:
            np.testing.assert_allclose(map@q,M@q,rtol=2e-13,atol=1e-14)
        f=rng.normal(size=(39,3))+1j*rng.normal(size=(39,3))
        np.testing.assert_allclose(map.T@f,M.T@f,rtol=2e-13,atol=1e-14)

    def test_loaded_motion_pressure_and_power_match_original_arithmetic(self):
        panel=plate.Panel();model=symmetric.solve(panel,9,8,plate.pilot_beams(panel));cells=[13,11]
        original=RitzSurface(model,cells);changed=RealArithmeticSurface(model,cells)
        B=plate.shape(model,[.32,.61,1.1],[.5,.5,.5]).T
        dynamic=MechanicalDynamic(model['frequency'],420.)
        old,s=solve_loaded(dynamic,CellAir(original,dynamic.s),B)
        new,t=solve_loaded(dynamic,CellAir(changed,dynamic.s),B)
        np.testing.assert_allclose(new,old,rtol=1e-10,atol=1e-16)
        np.testing.assert_allclose(changed.pressure(420,new,[[.55,.5,.66]]),original.pressure(420,old,[[.55,.5,.66]]),rtol=1e-10)
        np.testing.assert_allclose(t['air_power'],s['air_power'],rtol=1e-10)

if __name__=='__main__':unittest.main()
