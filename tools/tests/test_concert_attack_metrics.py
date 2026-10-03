"""Attack guards must distinguish envelopes with identical spectral magnitudes."""
import importlib.util
from pathlib import Path
import unittest

import numpy as np

spec=importlib.util.spec_from_file_location('attack_metrics',Path(__file__).resolve().parents[1]/'concert_grand_attack_metrics.py')
ATTACK=importlib.util.module_from_spec(spec); spec.loader.exec_module(ATTACK)


class TemporalAttack(unittest.TestCase):
    def test_time_reversal_has_same_magnitude_but_different_attack(self):
        rate=44100
        t=np.arange(round(.2*rate))/rate
        tone=(.1+.9*t/.2)*np.cos(2*np.pi*1000*t)
        reversed_tone=tone[::-1]
        np.testing.assert_allclose(abs(np.fft.rfft(tone)),abs(np.fft.rfft(reversed_tone)),atol=1e-10)
        padded=lambda y:np.repeat(np.r_[y,np.zeros(rate//2)][:,None],2,axis=1)
        target=ATTACK.measure(padded(tone),84,rate)
        actual=ATTACK.measure(padded(reversed_tone),84,rate)
        same=ATTACK.compare(target,target)
        different=ATTACK.compare(target,actual)
        self.assertGreater(different['envelope_shape_error_db'],5)
        self.assertFalse(ATTACK.guards({'note':different},{'note':same})['passed'])

    def test_relative_shape_is_gain_invariant_without_hiding_level_error(self):
        rate=44100
        t=np.arange(rate//2)/rate
        y=np.repeat((np.exp(-t/.1)*np.cos(2*np.pi*1000*t))[:,None],2,axis=1)
        target=ATTACK.measure(y,84,rate)
        prediction=ATTACK.measure(y*.1,84,rate)
        errors=ATTACK.compare(target,prediction)
        self.assertLess(errors['envelope_shape_error_db'],1e-10)
        self.assertLess(errors['rise_error_db'],1e-10)
        self.assertAlmostEqual(errors['attack_level_error_db'],20,places=9)


if __name__=='__main__': unittest.main()
