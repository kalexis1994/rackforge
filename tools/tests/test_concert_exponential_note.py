"""No spurious attack at onset/prefix/extension source joins."""
import sys,unittest
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.integrate import quad
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
spec=spec_from_file_location('exponential_note',Path(__file__).resolve().parents[1]/'render-concert-grand-exponential-note.py')
note=module_from_spec(spec);spec.loader.exec_module(note)
from concert_grand_cell_audio import hermite_fft,pressure_fft


class NoteJoinTests(unittest.TestCase):
    def fixture(self,linear=False):
        def fields(t):
            B=2*t if linear else .01+2*t+3*t*t+200*t**3
            V=np.full_like(t,2.) if linear else 2+6*t+600*t*t
            return B[:,None],V[:,None]
        onset_t=np.arange(513)/128000
        p_t=.004+np.arange(129)/64000;e_t=.006+np.arange(129)/64000
        onset=fields(onset_t);pB,pV=fields(p_t);eB,eV=fields(e_t)
        prefix=dict(board_position=pB,board_velocity=pV)
        extension=dict(board_position=eB,board_velocity=eV)
        return onset,prefix,extension,fields

    def test_subdivided_join_matches_continuous_cubic_acceleration(self):
        onset,prefix,extension,fields=self.fixture()
        t,B,V,_=note.joined_trace(*onset,prefix,extension,64000)
        expected_B,expected_V=fields(t)
        np.testing.assert_allclose(B,expected_B,rtol=2e-14,atol=1e-14)
        np.testing.assert_allclose(V,expected_V,rtol=2e-12,atol=1e-12)
        f,A=hermite_fft(t,B,V,2048)
        for i in [0,4,19,67,96]:
            expected=quad(lambda u:(6+1200*u)*np.cos(2*np.pi*f[i]*u),0,.008,epsabs=1e-12)[0]
            expected-=1j*quad(lambda u:(6+1200*u)*np.sin(2*np.pi*f[i]*u),0,.008,epsabs=1e-12)[0]
            self.assertLess(abs(A[i,0]-expected),2e-10)

    def test_constant_velocity_has_no_join_delta(self):
        onset,prefix,extension,_=self.fixture(True)
        t,B,V,_=note.joined_trace(*onset,prefix,extension,64000)
        kernel=np.zeros((16,1,1));kernel[4,0,0]=.3
        pressure,*_=pressure_fft(t,B,V,kernel)
        self.assertLess(np.max(abs(pressure)),1e-7)

    def test_discontinuous_source_is_rejected(self):
        onset,prefix,extension,_=self.fixture()
        extension['board_velocity'][0]+=1.
        with self.assertRaises(ValueError):note.joined_trace(*onset,prefix,extension,64000)


if __name__=='__main__':unittest.main()
