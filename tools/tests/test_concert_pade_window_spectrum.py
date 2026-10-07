"""Finite spectral observer against independent time-domain Padé replay."""
import sys
import unittest
from pathlib import Path
import numpy as np
from scipy.fft import rfft
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
from test_concert_pade_contact import fixture
from concert_grand_pade_window_spectrum import section_input_spectrum
from concert_grand_pade_air import PadeAir
from concert_grand_rational_air import RationalAir
from concert_grand_pade_midpoint import PadeMidpoint


class PadeWindowSpectrumTests(unittest.TestCase):
    def test_full_650_tap_count_with_cold_and_living_memory(self):
        rng=np.random.default_rng(650);H=np.zeros((650,4,3))
        H[:,::2]=rng.normal(size=(650,2,3))*.001
        H[-1,::2]=-H[:-1,::2].sum(axis=0)
        H[:,1::2]=-H[::-1,::2]
        wave=PadeAir(RationalAir(H,128000));initial=wave.zero_state()
        for rate in [1536000,3072000]:
            plan=PadeMidpoint(wave,1/rate);ports=rng.normal(size=(128,3))*.01
            final=initial.copy();history=[]
            for port in ports:
                final,inputs=plan.update(final,port);history.append(inputs)
            expected=rfft(np.asarray(history),n=192,axis=0);U=rfft(ports,n=192,axis=0)
            for first in range(0,97,8):
                last=min(first+8,97)
                actual=section_input_spectrum(wave,1/rate,initial,final,U,128,192,first,last)
                self.assertLess(np.linalg.norm(actual-expected[first:last])/max(np.linalg.norm(expected[first:last]),1e-30),2e-10)
            # Continue every section's memory instead of starting a second cold window.
            initial=final.copy()

    def test_all_sections_and_parseval_flux_with_live_nonperiodic_endpoints(self):
        rng=np.random.default_rng(10726);wave=fixture().air.wave
        for rate in [32000,1536000,3072000]:
            dt=1/rate;initial=rng.normal(size=wave.zero_state().shape)*.002
            ports=rng.normal(size=(37,wave.modes))*.03
            final=initial.copy();history=[]
            # Independent scalar section implementation, not the observer formula.
            for port in ports:
                final,_,inputs=wave.midpoint_deferred(final,port,dt);history.append(inputs)
            history=np.array(history);H=wave.base.H
            time_field=history.reshape(len(history),-1)@H.transpose(1,0,2).reshape(H.shape[1],-1).T
            time_energy=float(dt*np.sum(time_field*time_field)/2)
            for size in [37,64,96]:
                expected=rfft(history,n=size,axis=0);U=rfft(ports,n=size,axis=0)
                weights=np.full(size//2+1,2.);weights[0]=1.
                if size%2==0:weights[-1]=1.
                for block in [1,7,size//2+1]:
                    energy=0.
                    for first in range(0,size//2+1,block):
                        last=min(first+block,size//2+1)
                        actual=section_input_spectrum(wave,dt,initial,final,U,len(ports),size,first,last)
                        self.assertLess(np.linalg.norm(actual-expected[first:last])/max(np.linalg.norm(expected[first:last]),1e-30),2e-10)
                        field=actual.reshape(last-first,-1)@H.transpose(1,0,2).reshape(H.shape[1],-1).T
                        energy+=float(dt*np.sum(weights[first:last,None]*abs(field)**2)/(2*size))
                    self.assertLess(abs(energy-time_energy)/time_energy,2e-10)
                # Removing live boundary memories must not pass as a periodic window.
                wrong=section_input_spectrum(wave,dt,wave.zero_state(),wave.zero_state(),U,len(ports),size,0,size//2+1)
                self.assertGreater(np.linalg.norm(wrong-expected)/np.linalg.norm(expected),.01)

    def test_rejects_truncated_window_invalid_block_and_bad_port_shape(self):
        wave=fixture().air.wave;state=wave.zero_state();U=np.zeros((17,wave.modes),complex)
        for steps,size,first,last in [(33,32,0,17),(32,32,3,3),(32,32,0,18)]:
            with self.assertRaises(ValueError):section_input_spectrum(wave,1/32000,state,state,U,steps,size,first,last)
        with self.assertRaises(ValueError):section_input_spectrum(wave,1/32000,state,state,U[:,:1],32,32,0,17)


if __name__=='__main__':unittest.main()
