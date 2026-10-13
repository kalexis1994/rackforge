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

    def native(self,f,frames=16,*flags):
        path=self.folder/f'fixture-{len(list(self.folder.iterdir()))}.bin';write_fixture(path,f)
        return json.loads(subprocess.check_output([str(self.exe),str(path),str(frames),'8',*flags],text=True))

    def test_simd_matches_portable_without_changing_physical_terms(self):
        for nt in [4,96]:
            f=fixture(transverse=nt,seed=83)
            native=self.native(f);portable=self.native(f,16,'--portable')
            omega=np.asarray(native['omega'])
            self.assertLess(relative(response_metric(native,omega),response_metric(portable,omega)),1e-13)
            self.assertFalse(portable['simd'])
            for key in ['prepared_terms','prepared_bytes','endpoint_quadratic_gradient']:
                self.assertEqual(native[key],portable[key])
            self.assertEqual(native['allocations'],0);self.assertEqual(portable['allocations'],0)

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

    def test_maximum_modal_indices_match_sparse_continuous_force_integral(self):
        # Build the serialization directly, avoiding a dense 3073-coordinate
        # reference system. Only the final axial/transverse modes are excited.
        f=fixture(transverse=4);nt=256;nl=512;width=nl+2*nt
        f.update(transverse=nt,axial=nl,nodes=1024,shapes=np.zeros(3*nt),
            q=np.zeros(3*width+1),v=np.zeros(3*width+1),endpoint=np.zeros(3*width+1))
        selected=np.array([nl-1,nl+nt-1,width-1])
        f['q'][selected]=[2e-10,1e-8,-1.2e-8]
        f['v'][selected]=[.0001,.002,-.001]
        native=self.native(f);portable=self.native(f,16,'--portable')
        length,mu,young,density,rate=f['parameters'][:5]
        ea=young*mu/density;ei=young*(mu/density)**2/(4*np.pi)
        root=np.sqrt(mu*length/2)
        ku=np.pi*nl/length/root;kv=np.pi*nt/length/root
        axial=np.sqrt(ea*length/2)*ku
        transverse=np.sqrt(f['tensions'][0]*length/2*kv**2+
            ei*length/2*(np.pi*nt/length)**4/root**2)
        omega=np.array([axial,transverse,transverse]);h=16/rate
        points,weights=roots_legendre(4096);t=(points+1)*h/2;wt=weights*h/2
        qt=np.cos(t[:,None]*omega)*f['q'][selected]+np.sin(t[:,None]*omega)*f['v'][selected]/omega
        u,a,b=qt.T
        coefficient=ea*length/8*ku*kv**2
        gradient=coefficient*np.column_stack([a*a+b*b,2*u*a,2*u*b])
        expected_q=-np.sum(wt[:,None]*np.sin((h-t[:,None])*omega)/omega*gradient,axis=0)
        expected_v=-np.sum(wt[:,None]*np.cos((h-t[:,None])*omega)*gradient,axis=0)
        expected=np.r_[omega*expected_q,expected_v]
        for result in [native,portable]:
            observed=np.r_[omega*np.asarray(result['dq'])[selected],np.asarray(result['dv'])[selected]]
            self.assertLess(relative(observed,expected),1e-9)
            other=np.ones(3*width,dtype=bool);other[selected]=False
            np.testing.assert_array_equal(np.asarray(result['dq'])[other],np.zeros(sum(other)))
            np.testing.assert_array_equal(np.asarray(result['dv'])[other],np.zeros(sum(other)))
            self.assertEqual(result['allocations'],0)


if __name__=='__main__':unittest.main()
