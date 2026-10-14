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
        collocation=self.run_native(f,'verify',1,8,'--collocation')
        np.testing.assert_allclose(collocation['q'][:-1],expected_q,rtol=1e-10,atol=1e-18)
        np.testing.assert_allclose(collocation['v'][:-1],expected_v,rtol=1e-10,atol=1e-14)
        self.assertEqual(collocation['numerical_lambda_abs_max'],0.)
        self.assertEqual(collocation['maximum_iterations'],1);self.assertEqual(collocation['allocations'],0)

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

    def test_grouped_axial_waveform_preserves_scalar_trajectory(self):
        for nt,rate,frames,order in [(4,128000,16,64),(96,128000,16,96),(96,2048000,1,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            fast=self.run_native(f,'verify',frames,order)
            scalar=self.run_native(f,'verify',frames,order,'--scalar-axial-waveform')
            portable=self.run_native(f,'verify',frames,order,'--portable')
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            def metric(data):return np.r_[omega*np.asarray(data['q'])[:-1],data['v'][:-1]]
            for other in [scalar,portable]:
                self.assertTrue(fast['completed'] and other['completed'])
                self.assertLess(relative(metric(fast),metric(other)),1e-11)
                self.assertLess(other['work_relative_max'],1e-9)
                self.assertEqual(other['allocations'],0)
            self.assertEqual(fast['allocations'],0)
            # The diagnostic exercises both low/resonant and high branches at
            # multiple times, including their exactly-zero initial waveform.
            a=self.run_native(f,'waveform',frames,order)
            b=self.run_native(f,'waveform',frames,order,'--scalar-axial-waveform')
            self.assertLess(relative(np.asarray(a['driven_axial_q']),np.asarray(b['driven_axial_q'])),1e-12)

    def test_transverse_gradient_matches_full_and_independent_spatial_integral(self):
        for nt in [4,96]:
            f=self.coast(nt);data=self.run_native(f,'transverse-gradient')
            width=4*nt;nl=2*nt
            length,mu,young,density=f['parameters'][:4];ea=young*mu/density
            root=np.sqrt(mu*length/2);points,weights=roots_legendre(16);cells=2*nt
            x=((np.arange(cells)[:,None]+(points[None]+1)/2)*length/cells).ravel()
            wt=np.tile(weights*length/(2*cells),cells)
            du=np.cos(x[:,None]*np.pi*np.arange(1,nl+1)/length)*(np.pi*np.arange(1,nl+1)/length/root)
            dt=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*(np.pi*np.arange(1,nt+1)/length/root)
            expected=[];indices=[];axial=[]
            for i in range(3):
                q=f['q'][i*width:(i+1)*width];u=du @ q[:nl];v=dt @ q[nl:nl+nt];w=dt @ q[nl+nt:]
                eta=.5*(v*v+w*w)
                expected.extend(np.r_[np.zeros(nl),ea*dt.T @ (wt*(u+eta)*v),ea*dt.T @ (wt*(u+eta)*w)])
                indices.extend(range(i*width+nl,(i+1)*width));axial.extend(range(i*width,i*width+nl))
            np.testing.assert_allclose(data['gradient'],expected,rtol=2e-10,atol=1e-15)
            np.testing.assert_array_equal(np.asarray(data['gradient'])[indices],np.asarray(data['full_gradient'])[indices])
            np.testing.assert_array_equal(np.asarray(data['gradient'])[axial],np.zeros(len(axial)))
            np.testing.assert_array_equal(data['full_gradient_after'],data['full_gradient'])

    def test_transverse_sample_gradient_preserves_complete_trajectory(self):
        for nt,rate,frames,order in [(4,128000,16,64),(96,128000,16,96),(96,2048000,1,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            fast=self.run_native(f,'verify',frames,order)
            full=self.run_native(f,'verify',frames,order,'--full-sample-gradient')
            self.assertTrue(fast['completed'] and full['completed'])
            for key in ['q','v','work_relative_max','numerical_lambda_abs_max','maximum_iterations']:
                self.assertEqual(fast[key],full[key],key)
            self.assertEqual(fast['allocations'],0)

    def test_collocation_cardinal_integrals_match_polynomial_moments(self):
        for order in [8,16,32]:
            data=self.run_native(self.coast(),'collocation-rule',1,order)
            rule=np.asarray(data['rule']);t=rule[:,0]/data['h']
            a=np.asarray(data['matrix']).reshape(order,order)/data['h']
            for degree in range(order):
                np.testing.assert_allclose(a @ t**degree,t**(degree+1)/(degree+1),rtol=0,atol=3e-14)

    def test_collocation_matches_continuous_ode_without_energy_projection(self):
        f=self.coast(4,.0005);ns=len(f['q'])-1;core=f['system'].string
        def derivative(_t,y):
            return np.r_[y[ns:],-core.gradient_hessian(np.r_[y[:ns],0.],False)[:ns]]
        ode=solve_ivp(derivative,(0,.0005),np.r_[f['q'][:-1],f['v'][:-1]],
            method='DOP853',rtol=1e-12,atol=1e-15,max_step=1/2048000)
        self.assertTrue(ode.success)
        omega=np.sqrt(np.diag(core.K))[f['keep'][:-1]]
        expected=np.r_[omega*ode.y[:ns,-1],ode.y[ns:,-1]]
        errors=[]
        for frames in [8,4,2]:
            native=self.run_native(f,'verify',frames,8,'--collocation','--unconstrained')
            self.assertTrue(native['completed']);self.assertTrue(native['temporal_collocation'])
            errors.append(relative(np.r_[omega*np.asarray(native['q'])[:-1],native['v'][:-1]],expected))
            self.assertEqual(native['numerical_lambda_abs_max'],0.)
            self.assertEqual(native['allocations'],0)
        self.assertTrue(errors[0]>errors[1]>errors[2],errors)
        self.assertLess(errors[-1],1e-8)

    def test_collocation_failure_is_atomic_and_empty_wire_is_invariant(self):
        f=self.coast(12,.000125)
        failed=self.run_native(f,'verify',1,8,'--collocation','--iteration-limit=1')
        self.assertFalse(failed['completed']);self.assertEqual(failed['completed_microframes'],0)
        np.testing.assert_array_equal(failed['q'],f['q']);np.testing.assert_array_equal(failed['v'],f['v'])
        self.assertEqual(failed['allocations'],0)
        width=48;f['q'][:width]=0.;f['v'][:width]=0.
        native=self.run_native(f,'verify',1,8,'--collocation')
        self.assertTrue(native['completed'])
        np.testing.assert_array_equal(native['q'][:width],np.zeros(width))
        np.testing.assert_array_equal(native['v'][:width],np.zeros(width))
        initial=self.audit.wire_energies(f,f['q'],f['v'])
        final=self.audit.wire_energies(f,native['q'],native['v'])
        self.assertLess(max(abs(final-initial)/np.maximum(initial,1e-30)),1e-9)

    def test_diagnostic_profile_preserves_state_and_counts_without_allocations(self):
        exe=self.folder/'profile.exe';root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O','--cfg','concert_phase_library',
            '--cfg','concert_phase_profile',str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
        f=fixture(transverse=4,rate=128000,seed=83,duration=.000125);f['launch_speeds'][:]=0.
        plain=self.run_native(f,'verify',1,8,'--collocation')
        path=self.folder/'profile-input.bin';write_fixture(path,f)
        data=json.loads(subprocess.check_output([str(exe),str(path),'verify','1','8','--collocation'],text=True))
        self.assertIsNone(plain['profile'])
        for key in ['q','v','work_relative_max','numerical_lambda_abs_max',
                    'maximum_iterations','midpoint_residual_relative_max','known_prepared_bytes']:
            self.assertEqual(data[key],plain[key],key)
        self.assertEqual(data['allocations'],0)
        profile=data['profile'];sections={s['name']:s for s in profile['sections']}
        steps=data['completed_microframes'];self.assertEqual(steps,16)
        for name in ['step_energy_and_linear_setup','node_setup','endpoint','raw_state_and_energy',
                     'energy_projection','publish','ledger_monitor']:
            self.assertEqual(sections[name]['calls'],steps,name)
        self.assertGreaterEqual(sections['nonlinear_forces']['calls'],steps)
        self.assertEqual(sections['node_contraction']['calls'],sections['nonlinear_forces']['calls'])
        self.assertEqual(2*sections['residual']['calls'],sections['nonlinear_forces']['calls'])
        self.assertTrue(data['axial_elimination'])
        self.assertTrue(all(s['milliseconds']>=0 for s in profile['sections']))
        self.assertGreaterEqual(profile['unattributed_ms'],0.)
        self.assertAlmostEqual(sum(s['milliseconds'] for s in profile['sections']),profile['measured_ms'],places=6)

    def test_collocation_benchmark_uses_requested_physical_duration(self):
        f=self.coast(4,.000125)
        verify=self.run_native(f,'verify',1,8,'--collocation')
        bench=self.run_native(f,'bench',1,8,'8','--collocation','--buffer-microframes=2')
        self.assertTrue(bench['completed']);self.assertEqual(bench['completed_microframes'],16)
        self.assertEqual(bench['buffers'],8);self.assertEqual(bench['buffer_microframes'],2)
        self.assertEqual(bench['allocations'],0)
        np.testing.assert_array_equal(bench['q'],verify['q']);np.testing.assert_array_equal(bench['v'],verify['v'])

    def test_converged_node_force_reuse_preserves_state_and_wire_energy(self):
        for nt,rate,order in [(4,128000,8),(96,128000,24),(96,256000,16),(96,512000,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            reused=self.run_native(f,'verify',1,order,'--collocation')
            refreshed=self.run_native(f,'verify',1,order,'--collocation','--refresh-converged-forces')
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            def metric(d):return np.r_[omega*np.asarray(d['q'])[:-1],d['v'][:-1]]
            self.assertTrue(reused['completed'] and refreshed['completed'])
            self.assertLess(relative(metric(reused),metric(refreshed)),1e-10)
            initial=self.audit.wire_energies(f,f['q'],f['v'])
            final=self.audit.wire_energies(f,reused['q'],reused['v'])
            self.assertLess(max(abs(final-initial)/np.maximum(initial,1e-30)),1e-9)
            self.assertEqual(reused['allocations'],0)
            self.assertLess(reused['midpoint_residual_relative_max'],1e-11)

    def test_four_node_gradient_matches_single_and_independent_spatial_quadrature(self):
        for nt in [4,96,256]:
            if nt==256:
                f=self.coast(4);width=4*nt
                f.update(transverse=nt,axial=2*nt,nodes=1024,shapes=np.zeros(3*nt),
                         q=np.zeros(3*width+1),v=np.zeros(3*width+1),endpoint=np.zeros(3*width+1))
                f['q'][[2*nt-1,3*nt-1,width-1]]=[2e-10,1e-8,-1.2e-8]
            else:f=self.coast(nt)
            data=self.run_native(f,'batch-gradient');width=4*nt;nl=2*nt
            split=self.run_native(f,'batch-gradient',1,8,'--split-batch-forces')
            np.testing.assert_array_equal(split['gradient'],data['gradient'])
            legacy=self.run_native(f,'batch-gradient',1,8,'--legacy-batch-packing')
            np.testing.assert_array_equal(legacy['gradient'],data['single_gradient'])
            q=np.asarray(data['q_nodes']).reshape(4,3,width)
            observed=np.asarray(data['gradient']).reshape(4,3,width)
            single=np.asarray(data['single_gradient']).reshape(4,3,width)
            np.testing.assert_array_equal(observed[2],np.zeros((3,width)))
            self.assertGreater(np.linalg.norm(observed[3]),0.)
            length,mu,young,density=f['parameters'][:4];ea=young*mu/density;root=np.sqrt(mu*length/2)
            points,weights=roots_legendre(16);cells=2*nt
            x=((np.arange(cells)[:,None]+(points[None]+1)/2)*length/cells).ravel()
            wt=np.tile(weights*length/(2*cells),cells)
            du=np.cos(x[:,None]*np.pi*np.arange(1,nl+1)/length)*(np.pi*np.arange(1,nl+1)/length/root)
            dt=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*(np.pi*np.arange(1,nt+1)/length/root)
            for lane in [0,1,3]:
                for wire in range(3):
                    if np.linalg.norm(single[lane,wire])>0:
                        self.assertLess(relative(observed[lane,wire],single[lane,wire]),1e-13)
                    else:
                        np.testing.assert_array_equal(observed[lane,wire],single[lane,wire])
                    u=du @ q[lane,wire,:nl];v=dt @ q[lane,wire,nl:nl+nt];w=dt @ q[lane,wire,nl+nt:]
                    eta=.5*(v*v+w*w)
                    expected=np.r_[ea*du.T @ (wt*eta),ea*dt.T @ (wt*(u+eta)*v),ea*dt.T @ (wt*(u+eta)*w)]
                    if np.linalg.norm(expected)>0:
                        self.assertLess(relative(observed[lane,wire],expected),2e-9)
                    else:
                        self.assertTrue(np.all(observed[lane,wire]==0.))

    def test_batched_forces_preserve_full_trajectory_and_scalar_tail(self):
        for nt,rate,order in [(4,128000,10),(96,128000,24),(96,256000,16),(96,512000,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            batch=self.run_native(f,'verify',1,order,'--collocation')
            single=self.run_native(f,'verify',1,order,'--collocation','--scalar-collocation-forces')
            self.assertTrue(batch['completed'] and single['completed'])
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            def metric(d):return np.r_[omega*np.asarray(d['q'])[:-1],d['v'][:-1]]
            self.assertLess(relative(metric(batch),metric(single)),1e-9)
            initial=self.audit.wire_energies(f,f['q'],f['v'])
            final=self.audit.wire_energies(f,batch['q'],batch['v'])
            self.assertLess(max(abs(final-initial)/np.maximum(initial,1e-30)),1e-9)
            self.assertEqual(batch['allocations'],0)

    def test_axial_elimination_preserves_full_equations_and_state(self):
        for nt,rate,order in [(1,128000,8),(4,128000,10),(12,128000,8),
                              (96,128000,24),(96,256000,16),(96,512000,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            eliminated=self.run_native(f,'verify',1,order,'--collocation')
            audited=self.run_native(f,'verify',1,order,'--collocation','--audit-full-collocation-residual')
            simultaneous=self.run_native(f,'verify',1,order,'--collocation','--simultaneous-collocation')
            self.assertTrue(eliminated['completed'] and audited['completed'] and simultaneous['completed'])
            self.assertTrue(eliminated['axial_elimination']);self.assertFalse(simultaneous['axial_elimination'])
            for key in ['q','v','work_relative_max','numerical_lambda_abs_max',
                        'maximum_iterations','midpoint_residual_relative_max']:
                self.assertEqual(eliminated[key],audited[key],key)
            self.assertLessEqual(audited['audited_residual_relative_max'],1e-11)
            self.assertEqual(audited['midpoint_residual_relative_max'],audited['audited_residual_relative_max'])
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            def metric(d):return np.r_[omega*np.asarray(d['q'])[:-1],d['v'][:-1]]
            self.assertLess(relative(metric(eliminated),metric(simultaneous)),1e-9)
            initial=self.audit.wire_energies(f,f['q'],f['v'])
            final=self.audit.wire_energies(f,eliminated['q'],eliminated['v'])
            self.assertLess(max(abs(final-initial)/np.maximum(initial,1e-30)),1e-9)
            self.assertEqual(eliminated['allocations'],0);self.assertEqual(audited['allocations'],0)
            self.assertEqual(eliminated['known_prepared_bytes']-simultaneous['known_prepared_bytes'],3*3*f['nodes']*32*(order//4))
            if nt==96:self.assertLess(eliminated['maximum_iterations'],simultaneous['maximum_iterations'])
        f=self.coast(12,.000125)
        for flags in [[],['--simultaneous-collocation']]:
            data=self.run_native(f,'verify',1,8,'--collocation','--iteration-limit=2',*flags)
            self.assertFalse(data['completed']);self.assertEqual(data['completed_microframes'],0)
            np.testing.assert_array_equal(data['q'],f['q']);np.testing.assert_array_equal(data['v'],f['v'])

    def test_blocked_contraction_preserves_exact_state_work_and_failure(self):
        for nt,rate,order in [(1,128000,8),(4,128000,10),(12,128000,8),
                              (96,128000,24),(96,256000,16),(96,512000,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            blocked=self.run_native(f,'verify',1,order,'--collocation')
            streamed=self.run_native(f,'verify',1,order,'--collocation','--streamed-node-contraction')
            self.assertTrue(blocked['completed'] and streamed['completed'])
            for key in ['q','v','work_relative_max','numerical_lambda_abs_max',
                        'maximum_iterations','midpoint_residual_relative_max','known_prepared_bytes']:
                self.assertEqual(blocked[key],streamed[key],key)
            self.assertEqual(blocked['allocations'],0)
        f=self.coast(12,.000125)
        for flags in [[],['--streamed-node-contraction']]:
            data=self.run_native(f,'verify',1,8,'--collocation','--iteration-limit=2',*flags)
            self.assertFalse(data['completed']);self.assertEqual(data['completed_microframes'],0)
            np.testing.assert_array_equal(data['q'],f['q']);np.testing.assert_array_equal(data['v'],f['v'])
            self.assertEqual(data['allocations'],0)

    def test_prepared_packing_preserves_legacy_trajectory_and_work(self):
        for rate,order in [(128000,24),(256000,16),(512000,8)]:
            f=fixture(transverse=96,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            prepared=self.run_native(f,'verify',1,order,'--collocation')
            legacy=self.run_native(f,'verify',1,order,'--collocation','--legacy-batch-packing')
            omega=np.sqrt(np.diag(f['system'].string.K))[f['keep'][:-1]]
            def metric(d):return np.r_[omega*np.asarray(d['q'])[:-1],d['v'][:-1]]
            self.assertTrue(prepared['completed'] and legacy['completed'])
            self.assertLess(relative(metric(prepared),metric(legacy)),1e-9)
            self.assertLess(prepared['work_relative_max'],1e-9)
            self.assertLess(prepared['midpoint_residual_relative_max'],1e-11)
            self.assertEqual(prepared['allocations'],0)

    def test_block_residual_and_swapped_buffers_preserve_nodes_and_failures(self):
        for nt,rate,order in [(4,128000,10),(96,128000,24),(96,256000,16),(96,512000,8)]:
            f=fixture(transverse=nt,rate=rate,seed=83,duration=.000125);f['launch_speeds'][:]=0.
            fast=self.run_native(f,'verify',1,order,'--collocation')
            legacy=self.run_native(f,'verify',1,order,'--collocation','--legacy-node-updates')
            self.assertTrue(fast['completed'] and legacy['completed'])
            for key in ['q','v','work_relative_max','numerical_lambda_abs_max',
                        'maximum_iterations','midpoint_residual_relative_max']:
                self.assertEqual(fast[key],legacy[key],key)
            self.assertEqual(fast['allocations'],0)
        f=self.coast(12,.000125)
        for flags in [[],['--legacy-node-updates']]:
            # Two unsuccessful iterations exercise a workspace swap before
            # failure. No physical state from either iterate may be published.
            data=self.run_native(f,'verify',1,8,'--collocation','--iteration-limit=2',*flags)
            self.assertFalse(data['completed']);self.assertEqual(data['completed_microframes'],0)
            np.testing.assert_array_equal(data['q'],f['q']);np.testing.assert_array_equal(data['v'],f['v'])
            self.assertEqual(data['allocations'],0)

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
