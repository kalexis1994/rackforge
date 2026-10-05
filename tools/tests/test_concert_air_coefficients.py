"""Physical wave-port restriction and immutable archive selection checks."""
import sys,tempfile,unittest,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_air_coefficients import load_prefix
from concert_grand_rational_air import RationalAir


class CoefficientPrefixTests(unittest.TestCase):
    def fixture(self,folder,**override):
        rng=np.random.default_rng(628)
        raw=rng.normal(size=(8,3,3));raw[-1]=-raw[:-1].sum(axis=0)
        H=np.concatenate([raw,-raw[::-1]],axis=1)
        values=dict(factor=H,factor_rate=64000.,frequency_hz=np.array([100.,200.,300.]),static_mass=np.eye(3),high_limit=2*np.eye(3))
        values.update(override);path=Path(folder)/'parent.npz';np.savez(path,**values)
        return path,H

    def test_wave_impedance_is_the_physical_principal_port(self):
        with tempfile.TemporaryDirectory() as folder:
            path,H=self.fixture(folder);before=hashlib.sha256(path.read_bytes()).hexdigest()
            selected,rate,mass,limit,count=load_prefix(path,[100.,200.])
            full=RationalAir(H,rate);small=RationalAir(selected,rate)
            for s in [20+100j,2j*np.pi*261.,2j*np.pi*6000.]:
                np.testing.assert_allclose(small.impedance(s),full.impedance(s)[:2,:2],rtol=2e-13,atol=1e-13)
            self.assertEqual(count,3);np.testing.assert_array_equal(mass,np.eye(2));np.testing.assert_array_equal(limit,2*np.eye(2))
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())

    def test_zero_unused_velocity_preserves_wave_work_and_radiation(self):
        with tempfile.TemporaryDirectory() as folder:
            path,H=self.fixture(folder);selected,rate,*_=load_prefix(path,[100.,200.])
            full=RationalAir(H,rate);small=RationalAir(selected,rate);a=full.zero_state();b=small.zero_state()
            for v in [[.02,-.01],[-.03,.04],[0.,0.],[.01,.005]]:
                a,f,p=full.midpoint(a,np.r_[v,0.],1/192000)
                b,g,q=small.midpoint(b,np.array(v),1/192000)
                np.testing.assert_allclose(a[:,:2],b,rtol=0,atol=0)
                np.testing.assert_allclose(f[:2],g,rtol=2e-13,atol=1e-13)
                self.assertAlmostEqual(p,q,delta=1e-13)

    def test_reordered_or_unknown_frequencies_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path,_=self.fixture(folder)
            for f in [[200.,100.],[100.,201.],[100.,200.,300.,400.],[0.],[float('nan')]]:
                with self.assertRaises(ValueError):load_prefix(path,f)

    def test_inconsistent_parent_dimensions_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path,_=self.fixture(folder,static_mass=np.eye(2))
            with self.assertRaises(ValueError):load_prefix(path,[100.,200.])


if __name__=='__main__':unittest.main()
