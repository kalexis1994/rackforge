import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from scipy.linalg import block_diag
import concert_grand_orthotropic_plate as plate
import concert_grand_symmetric_plate as symmetric
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver
from concert_grand_matrixfree_air import RitzSurface,CellAir,MechanicalDynamic,solve_loaded,block_gmres
from concert_grand_compliant_projection import build


class MatrixfreeAirTests(unittest.TestCase):
    def fixture(self):
        model=symmetric.solve(plate.Panel(),7,6,plate.pilot_beams(plate.Panel()))
        surface=RitzSurface(model,[9,7]);dense=RectangularLoad.from_plate(model,9,7)
        return model,surface,dense

    def test_separable_map_exact_transpose_and_dense_cell_kernel(self):
        model,surface,dense=self.fixture();rng=np.random.default_rng(12)
        q=rng.normal(size=(surface.modes,3))+1j*rng.normal(size=(surface.modes,3))
        field=rng.normal(size=(9,7,3))+1j*rng.normal(size=(9,7,3))
        np.testing.assert_allclose(surface.forward(q),np.einsum('xym,mp->xyp',dense.shapes,q),rtol=1e-13,atol=1e-14)
        np.testing.assert_allclose(np.vdot(surface.forward(q),field),np.vdot(q,surface.adjoint(field)),rtol=1e-13)
        for s in [2j*np.pi*250,100+2j*np.pi*700]:
            np.testing.assert_allclose(CellAir(surface,s,12).force(q),dense.impedance(s,12)@q,rtol=2e-12,atol=1e-12)

    def test_loaded_gmres_matches_dense_solve_and_independent_power(self):
        model,surface,dense=self.fixture();B=plate.shape(model,[.3,.7,1.1],[.5,.5,.5]).T
        for hz in [100.,480.,1200.]:
            mech=MechanicalDynamic(model['frequency'],hz);air=CellAir(surface,mech.s,12)
            actual,stats=solve_loaded(mech,air,B)
            scalar,_=solve_loaded(mech,air,B,solver='gmres')
            np.testing.assert_allclose(actual,scalar,rtol=2e-8,atol=1e-15)
            expected=np.linalg.solve(np.diag(mech.diagonal)+mech.s*dense.impedance(mech.s,12),B)
            np.testing.assert_allclose(actual,expected,rtol=2e-8,atol=1e-15)
            self.assertLess(max(stats['force_residual']),2e-10)
            self.assertLess(max(stats['power_balance_relative']),1e-8)
            independent=surface.angular_power(hz,mech.s*actual,64,128)
            np.testing.assert_allclose(independent,stats['air_power'],rtol=1e-8,atol=1e-13)
            np.testing.assert_allclose(independent,surface.angular_power(hz,mech.s*actual,64,128,17),rtol=1e-13)
            observer=CellObserver(dense,[[.55,.5,.66]],4)
            np.testing.assert_allclose(surface.pressure(hz,actual,[[.55,.5,.66]]),mech.s**2*observer.acceleration_transfer(hz)[0]@actual,rtol=1e-12,atol=1e-14)

    def test_projected_loss_dynamic_against_full_dense_matrix(self):
        f=np.geomspace(80,20000,97);B=np.random.default_rng(42).normal(size=(len(f),3));r=build(f,B,2000)
        n=r['low_count'];Q=r['projection'];hz=5300.;dynamic=MechanicalDynamic(r['frequency'],hz,projected=r);s=dynamic.s
        w=2*np.pi*f;D=Q.T@np.diag(w*w+s*.046*w+s*s)@Q
        b=Q.T@B;actual=dynamic.solve(b)
        np.testing.assert_allclose(actual,np.linalg.solve(D,b),rtol=2e-10,atol=1e-16)
        np.testing.assert_allclose(dynamic.apply(actual),b,rtol=1e-10,atol=1e-12)
        v=s*actual;C=Q.T@(.046*w[:,None]*Q)
        np.testing.assert_allclose(dynamic.heat(v),np.sum((v.conj()*(C@v)).real,axis=0)/2,rtol=1e-12)

    def test_non_symmetric_coefficients_cannot_silently_drop_a_sector(self):
        full=plate.solve(plate.Panel(),5,6)
        with self.assertRaises(ValueError):RitzSurface(full,[7,5])

    def test_block_solver_independent_nonnormal_system_and_iteration_failure(self):
        rng=np.random.default_rng(652);A=np.eye(47)+.055*(rng.normal(size=(47,47))+1j*rng.normal(size=(47,47)))
        B=rng.normal(size=(47,4))+1j*rng.normal(size=(47,4))
        actual,steps=block_gmres(lambda q:A@q,B,max_steps=20)
        np.testing.assert_allclose(actual,np.linalg.solve(A,B),rtol=2e-8,atol=1e-10)
        self.assertGreater(steps,1)
        with self.assertRaises(RuntimeError):block_gmres(lambda q:A@q,B,max_steps=1)

if __name__=='__main__':unittest.main()
