import json
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from scipy.integrate import solve_ivp
from scipy.special import roots_legendre
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_phase_forces import quadratic_control,relative


class PhaseTrajectoryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.folder=Path(cls.temp.name);cls.exe=cls.folder/'trajectory.exe'
        root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O','--cfg','concert_phase_library',
            str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(cls.exe)],check=True)
        spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
        cls.audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.audit)

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def run_native(self,f,mode='verify',frames=16,order=64,*flags):
        path=self.folder/f'fixture-{len(list(self.folder.iterdir()))}.bin';write_fixture(path,f)
        return json.loads(subprocess.check_output([str(self.exe),str(path),mode,str(frames),str(order),*flags],text=True))

    def coast(self,nt=4,duration=.001):
        f=fixture(transverse=nt,seed=83,duration=duration);f['launch_speeds'][:]=0
        return f

    def test_cubic_gradient_matches_independent_spatial_gauss(self):
        for nt in [4,96]:
            f=self.coast(nt);native=self.run_native(f,'cubic')
            length,mu,young,density=f['parameters'][:4];ea=young*mu/density
            root=np.sqrt(mu*length/2);points,weights=roots_legendre(16);cells=2*nt
            x=((np.arange(cells)[:,None]+(points[None]+1)/2)*length/cells).ravel()
            wt=np.tile(weights*length/(2*cells),cells)
            dt=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*(np.pi*np.arange(1,nt+1)/length/root)
            expected=[]
            for i in range(3):
                q=f['q'][4*nt*i:4*nt*(i+1)];a=dt @ q[2*nt:3*nt];b=dt @ q[3*nt:]
                eta=.5*(a*a+b*b)
                expected.extend(np.r_[np.zeros(2*nt),ea*dt.T @ (wt*eta*a),ea*dt.T @ (wt*eta*b)])
            np.testing.assert_allclose(native['gradient'],expected,rtol=2e-10,atol=1e-15)

    def test_axial_waveform_reconstructs_independent_duhamel_integrals(self):
        for nt in [4,96]:
            f=self.coast(nt);native=self.run_native(f,'waveform')
            nl=2*nt;width=4*nt;omega=np.asarray(native['omega_axial'])
            keep=np.concatenate([np.arange(i*width,i*width+nl) for i in range(3)])
            self.assertTrue(all(0<count<nl for count in native['high_modes_per_wire']))
            for time,values in zip(native['times_s'],native['driven_axial_q']):
                if time==0:
                    np.testing.assert_allclose(values,0.,atol=1e-22);continue
                independent=quadratic_control(f,frames=time*f['parameters'][4],order=1024)
                observed=omega*np.asarray(values);expected=omega*independent['exact']['dq'][keep]
                self.assertLess(relative(observed,expected),1e-9,(nt,time))

    def test_axial_only_motion_keeps_exact_linear_carriers_and_zero_correction(self):
        f=self.coast();nt=4;width=16
        for key in ['q','v']:
            for wire in range(3):f[key][wire*width+8:(wire+1)*width]=0.
        native=self.run_native(f);duration=f['times'][-1]
        omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
        expected_q=np.cos(omega*duration)*f['q'][:-1]+np.sin(omega*duration)*f['v'][:-1]/omega
        expected_v=-omega*np.sin(omega*duration)*f['q'][:-1]+np.cos(omega*duration)*f['v'][:-1]
        np.testing.assert_allclose(native['q'][:-1],expected_q,rtol=1e-10,atol=1e-18)
        np.testing.assert_allclose(native['v'][:-1],expected_v,rtol=1e-10,atol=1e-14)
        self.assertEqual(native['numerical_lambda_abs_max'],0.)
        self.assertEqual(native['maximum_iterations'],1);self.assertEqual(native['allocations'],0)

    def test_failed_coupled_iteration_does_not_publish_partial_state(self):
        f=self.coast(12,.000125)
        native=self.run_native(f,'verify',16,64,'--iteration-limit=1')
        self.assertFalse(native['completed']);self.assertEqual(native['completed_microframes'],0)
        np.testing.assert_array_equal(native['q'],f['q']);np.testing.assert_array_equal(native['v'],f['v'])
        self.assertEqual(native['allocations'],0)

    def test_each_decoupled_wire_closes_its_own_energy(self):
        f=self.coast(96,.000125);initial=self.audit.wire_energies(f,f['q'],f['v'])
        native=self.run_native(f,'verify',16,96)
        shared=self.run_native(f,'verify',16,96,'--shared-energy-root')
        final=self.audit.wire_energies(f,native['q'],native['v'])
        shared_final=self.audit.wire_energies(f,shared['q'],shared['v'])
        self.assertTrue(native['per_wire_energy_constraint'])
        self.assertLess(max(abs(final-initial)/initial),1e-9)
        self.assertGreater(max(abs(shared_final-initial)/initial),1e-9)
        self.assertLess(shared['work_relative_max'],1e-9)
        self.assertEqual(native['allocations'],0)

    def test_empty_wire_remains_empty_while_other_wires_are_active(self):
        f=self.coast(12,.0005);width=48
        f['q'][:width]=0.;f['v'][:width]=0.
        native=self.run_native(f,'verify',16,64)
        self.assertTrue(native['completed'])
        np.testing.assert_array_equal(native['q'][:width],np.zeros(width))
        np.testing.assert_array_equal(native['v'][:width],np.zeros(width))
        self.assertEqual(native['initial_wire_energy_j'][0],0.)
        self.assertEqual(native['final_wire_energy_j'][0],0.)

    def test_coupled_trajectory_refines_against_continuous_ode(self):
        f=self.coast(4,.0005);ns=len(f['q'])-1;core=f['system'].string
        def derivative(_t,y):
            gradient=core.gradient_hessian(np.r_[y[:ns],0.],False)[:ns]
            return np.r_[y[ns:],-gradient]
        ode=solve_ivp(derivative,(0,.0005),np.r_[f['q'][:-1],f['v'][:-1]],
            method='DOP853',rtol=1e-11,atol=1e-14,max_step=1/2048000)
        self.assertTrue(ode.success)
        omega=np.sqrt(np.diag(core.K))[f['keep'][:-1]];expected=np.r_[omega*ode.y[:ns,-1],ode.y[ns:,-1]]
        errors=[]
        for frames in [4,2,1]:
            native=self.run_native(f,'verify',frames,32)
            self.assertTrue(native['completed']);self.assertLess(native['work_relative_max'],1e-9)
            actual=np.r_[omega*np.asarray(native['q'])[:-1],native['v'][:-1]]
            errors.append(relative(actual,expected))
        self.assertTrue(errors[0]>errors[1]>errors[2],errors)
        self.assertLess(errors[-1],1e-6)


if __name__=='__main__':unittest.main()
