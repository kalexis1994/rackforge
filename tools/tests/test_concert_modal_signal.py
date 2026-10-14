import unittest
import numpy as np
from concert_grand_modal_viability import common_signal, signal_errors


class ModalSignalTests(unittest.TestCase):
    def test_common_grid_preserves_known_tone_and_exposes_phase_and_lost_band(self):
        def trace(rate,high=False,phase=0.):
            t=np.arange(round(.096*rate)+1)/rate
            value=np.sin(2*np.pi*261*t+phase)
            if high:value+=.2*np.sin(2*np.pi*10000*t)
            return {'rows':np.column_stack([t,np.tile(value[:,None],(1,9))]).tolist()}
        fine=common_signal(trace(512000),512000)
        coarse=common_signal(trace(16000),16000)
        self.assertEqual(fine.shape,coarse.shape)
        self.assertLess(signal_errors(fine,coarse)['maximum_probe_velocity_relative'],.002)
        shifted=common_signal(trace(16000,phase=np.pi/2),16000)
        self.assertGreater(signal_errors(fine,shifted)['maximum_probe_velocity_relative'],1.)
        high=common_signal(trace(512000,high=True),512000)
        result=signal_errors(high,coarse)
        band=next(b for b in result['bands'] if b['low_hz']==6000)
        self.assertGreater(band['reference_energy_fraction'],.03)
        self.assertGreater(band['complex_spectrum_relative'],.99)
        self.assertGreater(result['maximum_probe_velocity_relative'],.18)


if __name__=='__main__':unittest.main()
