import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from scipy.special import roots_legendre
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_phase_forces import quadratic_control,response_metric,relative


class PhaseForcesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.folder=Path(cls.temp.name)
        cls.exe=cls.folder/'phase.exe';root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O',str(root/'tools/concert-grand-phase-forces.rs'),'-o',str(cls.exe)],check=True)

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def native(self,f,frames=16):
        path=self.folder/f'fixture-{len(list(self.folder.iterdir()))}.bin';write_fixture(path,f)
        return json.loads(subprocess.check_output([str(self.exe),str(path),str(frames),'8'],text=True))

    def test_harmonic_response_handles_zero_signed_and_resonant_frequencies(self):
        points,weights=roots_legendre(1024)
        for omega,nu,h in [(0.,0.,.01),(0.,-27000.,.001),(100.,0.,.001),
            (100.,100.,.001),(100.,-100.,.001),(100.,np.nextafter(100.,101.),.001),
            (1700.,-4900000.,.000125),(1700.,4900000.,.000125),(1.,1e6,.000125),
            (1700.,4900000.,1e-8)]:
            native=np.asarray(json.loads(subprocess.check_output([str(self.exe),'harmonic',str(omega),str(nu),str(h)],text=True)))
            t=(points+1)*h/2;wt=weights*h/2
            force=np.array([np.cos(nu*t),np.sin(nu*t)])
            qkernel=(h-t) if omega==0 else np.sin(omega*(h-t))/omega
            expected=np.array([force @ (wt*qkernel),force @ (wt*np.cos(omega*(h-t)))])
            # Absolute tolerances scaled to the respective integration kernels
            # also cover exact zeros without dividing by a cancelled response.
            np.testing.assert_allclose(native[0],expected[0],rtol=2e-10,atol=h*h*2e-12)
            np.testing.assert_allclose(native[1],expected[1],rtol=2e-10,atol=h*2e-12)

    def test_quadratic_convolution_matches_independent_spatial_and_time_gauss(self):
        for nt,frames in [(4,1),(4,64),(12,16),(96,16)]:
            f=fixture(transverse=nt,rate=128000,seed=83,duration=.006)
            native=self.native(f,frames)
            reference=quadratic_control(f,frames,order=1024,spatial_gauss=True)
            error=relative(response_metric(native,reference['omega']),reference['exact']['metric'])
            self.assertLess(error,1e-10,(nt,frames,error))
            np.testing.assert_allclose(native['omega'],reference['omega'],rtol=3e-15)
            np.testing.assert_allclose(native['endpoint_quadratic_gradient'],reference['endpoint_quadratic_gradient'],rtol=2e-10,atol=2e-12)
            self.assertEqual(native['allocations'],0)
            self.assertFalse(native['nonlinear_step'])

    def test_force_impulse_is_quadratic_and_keeps_both_axes(self):
        f=fixture(transverse=12,seed=83)
        initial=self.native(f)
        f['q'][:-1]*=-2;f['v'][:-1]*=-2
        scaled=self.native(f)
        np.testing.assert_allclose(scaled['dq'],4*np.asarray(initial['dq']),rtol=1e-14,atol=1e-25)
        np.testing.assert_allclose(scaled['dv'],4*np.asarray(initial['dv']),rtol=1e-14,atol=1e-25)
        # Swap the two transverse axes without touching the axial carriers.
        width=48
        for key in ['q','v']:
            for wire in range(3):
                a=slice(wire*width+24,wire*width+36);b=slice(wire*width+36,(wire+1)*width)
                old=f[key][a].copy();f[key][a]=f[key][b];f[key][b]=old
        swapped=self.native(f)
        for key in ['dq','dv']:
            expected=np.asarray(scaled[key]).reshape(3,48).copy()
            expected[:,24:36],expected[:,36:48]=expected[:,36:48].copy(),expected[:,24:36].copy()
            np.testing.assert_allclose(swapped[key],expected.ravel(),rtol=2e-14,atol=1e-25)

    def test_zero_force_and_zero_axial_carriers_preserve_structure(self):
        f=fixture(transverse=4,seed=None)
        native=self.native(f)
        np.testing.assert_array_equal(native['dq'],np.zeros(48))
        np.testing.assert_array_equal(native['dv'],np.zeros(48))
        for wire in range(3):
            f['q'][16*wire+8]=1e-7;f['v'][16*wire+9]=.001
        native=self.native(f)
        for wire in range(3):
            self.assertGreater(np.linalg.norm(native['dv'][16*wire:16*wire+8]),0.)
            np.testing.assert_array_equal(native['dv'][16*wire+8:16*(wire+1)],np.zeros(8))


if __name__=='__main__':unittest.main()
