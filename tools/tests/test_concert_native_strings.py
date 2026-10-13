import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from scipy.integrate import solve_ivp
from scipy.special import roots_legendre
from concert_grand_native_strings import fixture,write_fixture,replay


class NativeStringsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory()
        cls.folder=Path(cls.temp.name);cls.exe=cls.folder/'strings.exe'
        root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O',str(root/'tools/concert-grand-native-strings.rs'),'-o',str(cls.exe)],check=True)
        # Independent reference composes every carrier at every microstep,
        # rather than using the prepared block maps/summation by parts.
        source=(root/'tools/concert-grand-native-strings.rs').read_text()
        start=source.index('    pub fn step_ramp(')
        end=source.index('    /// Depth-three Anderson',start)
        ramp=source[start:end]
        first=ramp.index('                for i in 0..3 {',ramp.index('for frame in 0..frames'))
        last=ramp.index('                let micro =',first)
        ramp=ramp[:first]+"""                for j in 0..self.size {
                    self.block_force[j]=(1.-alpha)*self.ramp_start_force[j]+alpha*self.ramp_end_force[j]+self.ramp_correction[j];
                }
                self.candidate.copy_from_slice(&self.q);
"""+ramp[last:]
        first=ramp.index('                for i in 0..3 {',ramp.index('let micro ='))
        last=ramp.index('                stats.heat',first)
        ramp=ramp[:first]+"""                for j in 0..self.size {
                    let displacement=self.q[j]-self.candidate[j];
                    work+=self.block_force[j]*displacement;
                    self.ramp_weighted_displacement[j]+=alpha*displacement;
                }
"""+ramp[last:]
        ramp=ramp.replace('self.contact_with_held_nonlinearity(true)','self.contact_with_held_nonlinearity(false)')
        ramp=ramp.replace('for &j in &self.free_indices {','for &j in &self.free_indices[..0] {')
        reference=cls.folder/'micro-reference.rs'
        reference.write_text(source[:start]+ramp+source[end:],encoding='utf-8')
        cls.micro_exe=cls.folder/'micro-reference.exe'
        subprocess.run(['rustc','--edition=2021','-O',str(reference),'-o',str(cls.micro_exe)],check=True)


    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def run_native(self,f,mode,*extra,exe=None):
        path=self.folder/f'{f["transverse"]}-{f["parameters"][4]}-{mode}-{len(list(self.folder.iterdir()))}.bin'
        write_fixture(path,f)
        return json.loads(subprocess.check_output([str(exe or self.exe),str(path),mode,*map(str,extra)],text=True))

    def test_fft_avf_and_boundary_work_against_independent_gauss(self):
        for nt in [4,12,48,96]:
            f=fixture(transverse=nt,seed=67)
            native=self.run_native(f,'kernel')
            length,mu,young,density=f['parameters'][:4]
            ea=young*mu/density;mass=mu*length/2
            # Local Gauss cells avoid the conditioning of a single very-high
            # degree rule in the 96-mode cancellation probes.
            points,weights=roots_legendre(16)
            cells=2*nt
            x=((np.arange(cells)[:,None]+(points[None]+1)/2)*length/cells).ravel()
            weight=np.tile(weights*length/(2*cells),cells)
            ku=np.pi*np.arange(1,2*nt+1)/length/np.sqrt(mass)
            kv=np.pi*np.arange(1,nt+1)/length/np.sqrt(mass)
            du=np.cos(x[:,None]*np.pi*np.arange(1,2*nt+1)/length)*ku
            dv=np.cos(x[:,None]*np.pi*np.arange(1,nt+1)/length)*kv
            width=4*nt
            gradients=[];boundaries=[];p0=[];p1=[]
            for i in range(3):
                a=f['q'][i*width:(i+1)*width];b=f['endpoint'][i*width:(i+1)*width]
                b0,b1=f['parameters'][9:11]
                def fields(q,bridge):return du @ q[:2*nt],dv @ q[2*nt:3*nt]+bridge/length,dv @ q[3*nt:]
                def potential(q,bridge):
                    u,v,w=fields(q,bridge);eta=.5*(v*v+w*w)
                    return ea*weight @ (u*eta+.5*eta**2)
                grad=np.zeros(width);boundary=0.
                for alpha in [.5-.5/np.sqrt(3),.5+.5/np.sqrt(3)]:
                    u,v,w=fields(a+alpha*(b-a),b0+alpha*(b1-b0));eta=.5*(v*v+w*w)
                    grad+=.5*ea*np.r_[du.T @ (weight*eta),dv.T @ (weight*(u+eta)*v),dv.T @ (weight*(u+eta)*w)]
                    boundary+=.5*ea/length*(weight @ ((u+eta)*v))
                gradients.extend(grad);boundaries.append(boundary);p0.append(potential(a,b0));p1.append(potential(b,b1))
                observed=np.asarray(native['gradient'])[i*width:(i+1)*width] @ (b-a)+native['boundary_gradient'][i]*(b1-b0)
                self.assertAlmostEqual(observed,p1[-1]-p0[-1],delta=1e-11*max(abs(p0[-1]),abs(p1[-1]),1e-20))
            np.testing.assert_allclose(native['gradient'],gradients,rtol=2e-10,atol=2e-12)
            np.testing.assert_allclose(native['boundary_gradient'],boundaries,rtol=2e-11,atol=1e-14)
            np.testing.assert_allclose(native['potential0'],p0,rtol=2e-11,atol=1e-16)
            n=np.arange(1,nt+1)
            expected=mu*length*(-1.)**(n+1)/(n*np.pi*np.sqrt(mass))
            np.testing.assert_allclose(native['cross_mass'],expected,rtol=2e-15)
            self.assertAlmostEqual(native['lift_mass'],mu*length/3,places=15)

    def test_shared_contact_live_restrike_and_signed_energy_match_python(self):
        for rate,loss in [(64000,0.),(128000,.1)]:
            f=fixture(transverse=4,rate=rate,seed=83,loss=loss,second_speed=.1,duration=.016)
            native=self.run_native(f,'verify');reference=replay(f)
            np.testing.assert_allclose(native['q'],reference['q'],rtol=1e-7,atol=2e-12)
            np.testing.assert_allclose(native['v'],reference['v'],rtol=1e-7,atol=2e-9)
            np.testing.assert_allclose(native['force_trace'],reference['force_trace'],rtol=1e-7,atol=2e-8)
            np.testing.assert_allclose(native['launch_work_j'],reference['launch_work_j'],rtol=1e-9,atol=1e-12)
            self.assertLess(native['launch_work_j'][1],0.)
            self.assertLess(native['work_relative_max'],1e-9)
            self.assertLess(reference['work_relative'],1e-9)
            self.assertGreaterEqual(native['heat_j'],0.)
            np.testing.assert_allclose(native['heat_j'],reference['heat_j'],rtol=1e-8,atol=1e-12)

    def test_native_contact_refines_against_continuous_ode(self):
        f=fixture(transverse=4,rate=256000,duration=.002,second_time=.012)
        system=f['system'];n=system.size
        q=np.zeros(n);q[f['keep']]=f['q'];v=np.zeros(n)
        (q,v,_,_),_=system.launch((q,v,np.empty(0),np.empty(0)),60,f['launch_speeds'][0])
        def derivative(_t,y):
            q,v=y[:n],y[n:]
            gradient=np.r_[system.string.gradient_hessian(q[:-1],False),0.]
            for _,d,k in system.contacts:
                compression=max(d @ q,0.)
                force=k*compression**system.felt.exponent*np.exp(np.clip(system.felt.rate_damping_s_per_m*(d @ v),-40,40))
                gradient+=d*force
            return np.r_[v,-gradient]
        ode=solve_ivp(derivative,(0,.002),np.r_[q,v],method='DOP853',rtol=1e-11,atol=1e-14,max_step=1/1536000)
        self.assertTrue(ode.success)
        omega=np.sqrt(np.diag(system.string.K))
        def metric(q,v):return np.r_[omega*q[:-1],v,q[-1]]
        target=metric(ode.y[:n,-1],ode.y[n:,-1]);errors=[]
        for rate in [64000,128000,256000]:
            case=fixture(transverse=4,rate=rate,duration=.002)
            result=self.run_native(case,'verify')
            q=np.zeros(n);v=q.copy();q[case['keep']]=result['q'];v[case['keep']]=result['v']
            errors.append(np.linalg.norm(metric(q,v)-target)/np.linalg.norm(target))
        self.assertTrue(errors[0]>3*errors[1] and errors[1]>3*errors[2],errors)
        self.assertLess(errors[-1],1e-4)

    def test_nonlinear_block_one_reproduces_monolithic_contact(self):
        f=fixture(transverse=4,rate=128000,seed=83,second_speed=.1,duration=.016)
        original=self.run_native(f,'verify')
        blocked=self.run_native(f,'verify-block',1)
        np.testing.assert_allclose(blocked['q'],original['q'],rtol=1e-7,atol=2e-12)
        np.testing.assert_allclose(blocked['v'],original['v'],rtol=1e-7,atol=2e-9)
        np.testing.assert_allclose(blocked['force_trace'],original['force_trace'],rtol=1e-7,atol=2e-8)
        self.assertLess(blocked['work_relative_max'],1e-9)

    def test_nonlinear_blocks_preserve_work_and_end_at_unaligned_restrike(self):
        f=fixture(transverse=4,rate=128000,seed=83,second_speed=.1,second_time=1417/128000,duration=.016)
        for frames in [8,16,32]:
            blocked=self.run_native(f,'verify-block',frames)
            self.assertLess(blocked['work_relative_max'],1e-9)
            self.assertLess(blocked['launch_work_j'][1],0.)
            self.assertEqual(len(blocked['force_trace']),768)


    def test_ramp_composes_every_carrier_and_preserves_unaligned_restrike(self):
        f=fixture(transverse=4,rate=128000,seed=83,second_speed=.1,second_time=1417/128000,duration=.016)
        for frames in [1,16,64]:
            native=self.run_native(f,'verify-ramp',frames)
            micro=self.run_native(f,'verify-ramp',frames,exe=self.micro_exe)
            np.testing.assert_allclose(native['q'],micro['q'],rtol=2e-8,atol=2e-12)
            np.testing.assert_allclose(native['v'],micro['v'],rtol=2e-8,atol=2e-9)
            np.testing.assert_allclose(native['force_trace'],micro['force_trace'],rtol=2e-8,atol=2e-8)
            np.testing.assert_allclose(native['heat_j'],micro['heat_j'],rtol=2e-9,atol=1e-12)
            self.assertLess(native['work_relative_max'],1e-9)
            self.assertLess(native['launch_work_j'][1],0.)
            self.assertEqual(len(native['force_trace']),768)



if __name__=='__main__':unittest.main()
