import sys,unittest
from pathlib import Path
import numpy as np
from scipy.linalg import expm
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_shared_strings import WirePort,SharedStrings,SharedHammers
from concert_grand_port_coast import PortCoast
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt
from concert_grand_rational_air import RationalAir
from concert_grand_pade_air import PadeAir,PadeBoundedAir


class PortTests(unittest.TestCase):
    def make(self,nonlinear):
        ports=[WirePort(60,Wire(.62,.0081),[1.7,-.9,.4]),WirePort(67,Wire(.42,.0081),[1.4,-.8,.5])]
        core=SharedStrings(ports,[110.,270.,680.],np.zeros((3,3)),4,8,nonlinear)
        system=SharedHammers(core,Felt(.002,7.5e8,2.6,.125,.02,.1))
        wave=PadeAir(RationalAir(np.zeros((3,6,3)),128000))
        air=PadeBoundedAir(wave,np.eye(3)*.001,np.eye(3)*.01)
        return core,system,PortCoast(system,air)

    def test_reconstructed_quadratic_and_schur_match_dense_matrices(self):
        core,system,pilot=self.make(True);rng=np.random.default_rng(31)
        a=rng.normal(size=core.size)
        recovered=pilot.unproject(pilot.omega2*pilot.project(a)+pilot.coupling_gradient(pilot.project(a)))
        np.testing.assert_allclose(recovered,core.K@a,atol=2e-6,rtol=1e-12)
        _,_,Q,_=pilot.oscillator_functions(1/16000)
        coupling=np.column_stack([pilot.coupling_gradient(x) for x in np.eye(core.size)])
        matrix=np.eye(core.size)+Q[:,None]*coupling/2
        np.testing.assert_allclose(pilot.solve_remainder(a,1/16000,Q),np.linalg.solve(matrix,a),atol=2e-15,rtol=1e-13)

    def test_full_nonlinear_mechanical_energy_conserved(self):
        core,system,pilot=self.make(True);rng=np.random.default_rng(32)
        q,v=system.initial_state(0.);q[:core.size]=rng.normal(size=core.size)*1e-6
        v[:core.size]=rng.normal(size=core.size)*.001
        initial=system.energy(q,v)
        for _ in range(200):q,v,_=pilot.mechanical_step(q,v,1/32000)
        self.assertLess(abs(system.energy(q,v)-initial)/initial,1e-9)

    def test_linear_waveform_converges_against_independent_matrix_exponential(self):
        core,system,pilot=self.make(False)
        q,v=system.initial_state(0.);q[core.entries[0]['sv'].start]=1e-6
        n=core.size;duration=.004
        reference=expm(np.block([[np.zeros((n,n)),np.eye(n)],[-core.K,np.zeros((n,n))]])*duration)@np.r_[q[:n],v[:n]]
        errors=[]
        for rate in [16000,32000,64000]:
            a,b=q.copy(),v.copy()
            for _ in range(round(duration*rate)):a,b,_=pilot.mechanical_step(a,b,1/rate)
            errors.append(np.linalg.norm(np.r_[a[:n],b[:n]]-reference)/np.linalg.norm(reference))
        self.assertTrue(all(errors[i]>3*errors[i+1] for i in [0,1]),errors)
        self.assertLess(errors[-1],.001)


if __name__=='__main__':unittest.main()
