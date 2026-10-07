"""Coast capture closes independent live wave work and rejects corrupted flux."""
import sys
import unittest
from pathlib import Path
import numpy as np

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(Path(__file__).resolve().parent))
from test_concert_pade_contact import fixture
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_coast_measurements import run_coast,field_balance
from concert_grand_pade_midpoint import PadeMidpoint


class CoastMeasurementTests(unittest.TestCase):
    def test_captured_ports_close_live_storage_and_independent_flux(self):
        ref=fixture();pilot=ExponentialCoast(ref.system,ref.air)
        rng=np.random.default_rng(61009)
        state=ref.initial_state(0.)
        state[0][:-1]=rng.normal(size=pilot.n-1)*1e-7;state[0][-1]=-.001
        state[1][:-1]=rng.normal(size=pilot.n-1)*.005;state[1][-1]=-.1
        state[2][:]=rng.normal(size=state[2].shape)*1e-5;state[3][:]=[.01,-.02,.03]
        original=tuple(x.copy() for x in state)
        final,row,ports,t,B,V=run_coast(pilot,state,128000,.00025)
        plan=PadeMidpoint(ref.air.wave,1/128000);memory=state[2].copy();flux=0.
        for v in ports:
            memory,u=plan.update(memory,v)
            flux+=ref.air.wave.radiation_batch(u[None])[0]/128000
        np.testing.assert_array_equal(memory,final[2])
        for a,b in zip(state,original):np.testing.assert_array_equal(a,b)
        self.assertEqual(B.shape,(33,ref.air.modes));self.assertEqual(V.shape,B.shape)
        np.testing.assert_allclose(t,np.arange(33)/128000)
        interval=lambda e:dict(lower_j=e,estimate_j=e,upper_j=e)
        before,after=interval(ref.air.wave.energy(state[2])),interval(ref.air.wave.energy(final[2]))
        checked=field_balance(row,before,after,interval(flux))
        self.assertLess(checked['total_budget_bound_relative'],1e-9)
        self.assertLess(checked['air_budget_bound_relative'],1e-9)
        # Radiated energy must be independently supplied, not derived from
        # the other work values to make a corrupted record balance.
        wrong=field_balance(row,before,after,interval(flux+1e-6))
        self.assertGreater(wrong['air_budget_bound_relative'],1e-6)
        with self.assertRaises(ValueError):
            field_balance(row,before,after,dict(lower_j=1.,estimate_j=0.,upper_j=2.))

    def test_capture_rejects_contact_and_bad_duration(self):
        ref=fixture();pilot=ExponentialCoast(ref.system,ref.air)
        state=ref.initial_state(0.)
        with self.assertRaises(ValueError):run_coast(pilot,state,128000,.00025)
        state[0][-1]=-.001
        with self.assertRaises(ValueError):run_coast(pilot,state,128000,.000251)


if __name__=='__main__':unittest.main()
