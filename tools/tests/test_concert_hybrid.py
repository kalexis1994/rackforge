import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_modal_viability import modal_indices,project,omega
from concert_grand_hybrid import high_signal


class HybridTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.folder=Path(cls.temp.name);cls.exe=cls.folder/'hybrid.exe'
        root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O','--cfg','concert_phase_library',
            str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(cls.exe)],check=True)

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def run_native(self,f,*flags):
        path=self.folder/f'fixture-{len(list(self.folder.iterdir()))}.bin';write_fixture(path,f)
        return json.loads(subprocess.check_output([str(self.exe),str(path),'verify','1','16','--collocation',*flags],text=True))

    def coast(self):
        f=fixture(transverse=12,rate=128000,seed=83,duration=.000125)
        f['launch_speeds'][:]=0.;return f

    def test_retaining_all_nonlinear_modes_matches_original_exactly(self):
        f=self.coast();a=self.run_native(f);b=self.run_native(f,'--linear-highs=12')
        for key in ['q','v']:self.assertEqual(a[key],b[key])
        self.assertEqual(b['allocations'],0)

    def test_low_subsystem_matches_projection_and_highs_are_exact_carriers(self):
        f=self.coast();low=4;small,indices=project(f,low,128000)
        a=self.run_native(small);b=self.run_native(f,'--linear-highs=4')
        for key in ['q','v']:np.testing.assert_array_equal(np.asarray(b[key])[indices],a[key][:-1])
        high=np.setdiff1d(np.arange(len(f['q'])-1),indices);w=omega(f)[high];h=f['times'][2]
        q=f['q'][high];v=f['v'][high];c=np.cos(w*h);s=np.sin(w*h)
        np.testing.assert_allclose(np.asarray(b['q'])[high],c*q+s*v/w,rtol=2e-12,atol=1e-20)
        np.testing.assert_allclose(np.asarray(b['v'])[high],-w*s*q+c*v,rtol=2e-12,atol=1e-15)
        self.assertLess(b['work_relative_max'],1e-9);self.assertEqual(b['allocations'],0)
        self.assertEqual(len(b['q']),len(f['q']))

    def test_failure_does_not_advance_any_coordinate(self):
        f=self.coast();a=self.run_native(f,'--linear-highs=4','--iteration-limit=1')
        self.assertFalse(a['completed']);self.assertEqual(a['completed_microframes'],0)
        for key in ['q','v']:np.testing.assert_array_equal(a[key],f[key])
        self.assertEqual(a['allocations'],0)

    def test_dense_observation_matches_independent_linear_modal_sum(self):
        f=self.coast();nt=f['transverse'];low=4;rate=128000
        observed=high_signal(f,low,rate);w=omega(f);length,mu=f['parameters'][:2]
        for step in [0,3,len(observed)-1]:
            for wire in range(3):
                for probe,x in enumerate([.125,.31,.73]):
                    expected=0.
                    for k in range(low,nt):
                        j=wire*4*nt+2*nt+k;n=k+1;t=step/rate
                        v=np.cos(w[j]*t)*f['v'][j]-w[j]*np.sin(w[j]*t)*f['q'][j]
                        expected+=v*np.sin(np.pi*n*x)*np.exp(-1.2*(n*.02)**2)/np.sqrt(mu*length/2)
                    self.assertAlmostEqual(observed[step,wire*3+probe],expected,places=13)


if __name__=='__main__':unittest.main()
