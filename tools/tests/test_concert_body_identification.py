"""Independent analytic reference for the causal identification kernel."""
import importlib.util
from pathlib import Path
import unittest

import numpy as np


class CausalBodyKernel(unittest.TestCase):
    def test_known_highpass_impulse_and_zero_dc(self):
        path=Path(__file__).resolve().parents[1]/'identify-concert-grand-body.py'
        spec=importlib.util.spec_from_file_location('body_kernel_test',path)
        body=importlib.util.module_from_spec(spec); spec.loader.exec_module(body)
        # H(z)=b(1-z^-1)/(1-a*z^-1) has a known causal impulse:
        # h[0]=b, h[n]=b(a-1)a^(n-1). It exercises the reconstruction and
        # truncation without comparing the FFT with its own inverse.
        pole=np.exp(-2*np.pi*20/44100)
        scale=(1+pole)/2
        z=np.exp(-2j*np.pi*body.FREQ/44100)
        target_db=20*np.log10(abs(scale*(1-z)/(1-pole*z)))
        gain=np.tile(target_db[None,:,None],(3,1,2))
        kernel,checks=body.kernels(gain)
        expected=np.r_[scale,scale*(pole-1)*pole**np.arange(body.TAPS-1)]
        for r in range(3):
            for side in range(2):
                relative=np.linalg.norm(kernel[r,side]-expected)/np.linalg.norm(expected)
                self.assertLess(relative,1e-10)
        self.assertLess(checks['maximum_dc_gain'],1e-10)
        self.assertLess(max(checks['discarded_tail_energy_db']),-150)


if __name__=='__main__': unittest.main()
