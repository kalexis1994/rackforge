"""Independent continuous transforms and retarded pressure checks."""
import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_cell_observer import CellObserver,acceleration_spectrum
from concert_grand_cell_audio import hermite_fft,retarded_kernel,kernel_transfer,pressure_fft


class CellAudioTests(unittest.TestCase):
    def test_fast_uniform_transform_matches_analytic_source_window(self):
        t=np.arange(1025)/128000
        B=np.c_[np.sin(2*np.pi*261*t)/1000,.01+2*t+3*t*t+200*t**3]
        V=np.c_[2*np.pi*261*np.cos(2*np.pi*261*t)/1000,2+6*t+600*t*t]
        f,A=hermite_fft(t,B,V,2048);indices=[0,4,19,67,96]
        expected=acceleration_spectrum(t,B,V,f[indices])
        np.testing.assert_allclose(A[indices],expected,rtol=2e-11,atol=2e-12)
        np.testing.assert_allclose(A[0],V[-1]-V[0],atol=1e-12)

    def test_fractional_retarded_cells_against_continuous_green_transfer(self):
        surface=RectangularLoad(.31,.21,np.array([[[1.,-.8]],[[.4,.2]],[[.7,.6]]]))
        observer=CellObserver(surface,[(.13,.09,.6),(.2,.14,1.2)],4)
        f=np.array([0.,261.,800.,1200.,2000.,4500.,6000.])
        kernel=retarded_kernel(observer,128000)
        actual=kernel_transfer(kernel,128000,f);expected=observer.acceleration_transfer(f)
        for a,b in zip(actual,expected):self.assertLess(np.linalg.norm(a-b)/np.linalg.norm(b),.0002)
        np.testing.assert_allclose(actual[0],expected[0],rtol=3e-14,atol=1e-14)
        self.assertTrue(np.all(kernel[:int(observer.distance.min()/343*128000)-1]==0))

    def test_pressure_scale_and_no_endpoint_delta_from_constant_velocity(self):
        t=np.arange(1025)/128000;B=(.01+2*t)[:,None];V=np.full_like(B,2.)
        kernel=np.zeros((16,1,1));kernel[4,0,0]=.3
        pressure,_,_,_=pressure_fft(t,B,V,kernel)
        np.testing.assert_allclose(pressure,0.,atol=1e-7)
        B=(t*t/2)[:,None];V=t[:,None]
        pressure,_,_,_=pressure_fft(t,B,V,kernel)
        # Inside the constant-acceleration window, excluding the Fourier
        # representation of its discontinuous boundaries, p = .3 Pa.
        self.assertLess(np.max(abs(pressure[100:900,0]-.3)),.002)

    def test_nonuniform_or_short_fft_rejected(self):
        with self.assertRaises(ValueError):hermite_fft([0,.1,.21],np.zeros((3,1)),np.zeros((3,1)),8)
        with self.assertRaises(ValueError):hermite_fft([0,.1,.2],np.zeros((3,1)),np.zeros((3,1)),2)


if __name__=='__main__':unittest.main()
