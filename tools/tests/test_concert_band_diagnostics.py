import sys, unittest
from pathlib import Path
import numpy as np
from numpy.polynomial.legendre import leggauss
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_band_diagnostics import frequency_grid,pressure_spectra,capture,compare_spectra,BANDS


class BandTests(unittest.TestCase):
    def test_acceleration_window_against_independent_gauss_integration(self):
        t=np.arange(257)/128000; x=t[:,None]
        B=.2*x+17*x*x+1e4*x**3; V=.2+34*x+3e4*x*x
        f=np.array([60.,2700.,9100.]); H=np.ones((3,1,1),complex)
        actual=pressure_spectra((t,B,V),f,H)
        nodes,weights=leggauss(48); u=(nodes+1)/2
        time=t[:-1,None]+np.diff(t)[:,None]*u
        acceleration=34+6e4*time
        for label in ['rectangular','hann']:
            window=np.ones_like(time) if label=='rectangular' else np.sin(np.pi*time/t[-1])**2
            values=acceleration*window*np.diff(t)[:,None]*weights/2
            expected=np.array([np.sum(values*np.exp(-2j*np.pi*hz*time)) for hz in f])
            np.testing.assert_allclose(actual[label][:,0],expected,rtol=1e-10,atol=2e-13)
        # Tapering position would create a nonzero acceleration in this case.
        linear=(t,t[:,None]*.3,np.full((len(t),1),.3))
        np.testing.assert_allclose(pressure_spectra(linear,f,H)['hann'],0.,atol=1e-12)

    def test_capture_subsamples_and_refines_the_same_cubic(self):
        for rate in [32000,256000]:
            t=np.arange(round(.002*rate)+1)/rate
            B=(3*t+6*t*t+4*t**3)[:,None]; V=(3+12*t+12*t*t)[:,None]
            new=capture((t,B,V))
            np.testing.assert_allclose(new[1][:,0],3*new[0]+6*new[0]**2+4*new[0]**3,rtol=1e-14,atol=1e-17)
            np.testing.assert_allclose(new[2][:,0],3+12*new[0]+12*new[0]**2,rtol=1e-13,atol=1e-13)
        self.assertEqual(len(new[0]),257)

    def test_nested_grid_and_complex_error_keep_gain_and_phase(self):
        f=frequency_grid(); coarse=f[::2]
        for low,high in BANDS:
            self.assertIn(low,coarse); self.assertIn(high,coarse)
        reference=np.column_stack([np.ones(len(f)),1j*np.ones(len(f))])
        factor=1.2*np.exp(.1j)
        result=compare_spectra(f,reference*factor,reference)
        for row in result['bands']: self.assertAlmostEqual(row['relative'],abs(factor-1),places=13)
        self.assertFalse(result['all_bands_1_percent_passed'])
        with self.assertRaises(ValueError): frequency_grid(0.)

    def test_refining_coarse_hermite_does_not_restore_a_high_carrier(self):
        # Dynamics is exact here: both grids sample the same 6 kHz oscillator.
        # Its cubic coarse reconstruction creates a 10 kHz image. Refining
        # that cubic preserves the image; it does not recover the sinusoid.
        omega=2*np.pi*6000
        t=np.arange(4097)/512000
        trace=(t,1e-5*np.sin(omega*t[:,None]),1e-5*omega*np.cos(omega*t[:,None]))
        coarse=capture(trace,16000)
        f=np.array([6000.,10000.]); H=np.ones((2,1,1),complex)
        fine=pressure_spectra(trace,f,H)['hann']
        native=pressure_spectra(coarse,f,H)['hann']
        refined=pressure_spectra(capture(coarse),f,H)['hann']
        np.testing.assert_allclose(native,refined,rtol=1e-10,atol=1e-10)
        self.assertGreater(abs(native[1,0]),.01*abs(native[0,0]))
        self.assertLess(abs(fine[1,0]),1e-6*abs(fine[0,0]))


if __name__=='__main__': unittest.main()
