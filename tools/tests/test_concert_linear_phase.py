import sys,unittest
from pathlib import Path
import numpy as np
from scipy.linalg import expm
from scipy.integrate import solve_ivp
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_linear_phase import LinearBank,gauss4_maps


class LinearPhaseTests(unittest.TestCase):
    def test_energy_coordinates_continuous_losses_and_actual_linear_map(self):
        bank=LinearBank(4,8); rng=np.random.default_rng(91)
        x=rng.normal(size=bank.size)*1e-7
        q,v,r,z=bank.state(x)
        E=bank.system.energy(q,v)+bank.air.energy((r,z))
        self.assertLess(abs(E/(x@x/2)-1),1e-11)
        p=sum(bank.powers(x).values())
        self.assertLess(abs(x@bank.L@x+p)/max(p,1e-30),1e-10)
        for label in bank.pilots:
            M=bank.transition(label,1/64000)
            np.testing.assert_allclose(M@x,bank.actual_step(label,x,1/64000),rtol=1e-9,atol=1e-19)
        y=bank.inverse@x
        ode=solve_ivp(lambda t,u:bank.rhs_raw(u),(0,1/64000),y,
                      method='DOP853',rtol=1e-11,atol=1e-17,max_step=1/1024000)
        self.assertTrue(ode.success)
        np.testing.assert_allclose(expm(bank.L/64000)@x,bank.root@ode.y[:,-1],rtol=1e-8,atol=1e-18)

    def test_gauss_stages_work_identity_order_and_exact_dry_carriers(self):
        bank=LinearBank(4,8); rng=np.random.default_rng(74)
        x=rng.normal(size=len(bank.coupled))*1e-7; errors=[]
        # The acoustic sections have poles of magnitude ~443k rad/s.
        # Verify the asymptotic order at h*|pole|<1, not in the stiff regime.
        for rate in [2048000,4096000,8192000]:
            dt=1/rate; M,stages=gauss4_maps(bank.Lc,dt)
            y=M@x; heat=0.
            for Y in stages:
                full=np.zeros(bank.size);full[bank.coupled]=Y@x
                heat+=dt/2*sum(bank.powers(full).values())
            self.assertLess(abs((y@y-x@x)/2+heat)/(x@x/2),1e-10)
            self.assertLessEqual(np.linalg.norm(y),np.linalg.norm(x)*(1+1e-12))
            errors.append(np.linalg.norm(y-expm(dt*bank.Lc)@x))
        self.assertTrue(errors[0]>20*errors[1]>400*errors[2],errors)
        M=bank.transition('gauss4',1/64000)
        np.testing.assert_allclose(M[np.ix_(bank.dry,bank.dry)],
                                   expm(bank.L[np.ix_(bank.dry,bank.dry)]/64000),rtol=1e-13,atol=1e-15)

    def test_midpoint_frequency_warp_matches_live_discrete_air_states(self):
        bank=LinearBank(4,8);air=bank.air;na=bank.nr+bank.nz
        def impedance(s):
            b=air.relaxation.B;tau=air.relaxation.tau
            return air.wave.impedance(s)+(b*(s*tau/(1+s*tau)))@b.T
        for rate in [32000,128000]:
            dt=1/rate;cache=bank.pilots['split']._air_prepare(dt)
            def discrete(a,u):
                r=a[:bank.nr].reshape(bank.shape);z=a[bank.nr:]
                constant,offsets=cache['wave_plan'].constant(r)
                constant-=air.relaxation.B@(z/(1+cache['relaxation_alpha']))
                r1,_=cache['wave_plan'].update(r,u,offsets)
                z1,_,_=air.relaxation.midpoint(z,u,dt)
                return np.r_[r1.ravel(),z1],constant+cache['A']@u
            columns=[discrete(e,np.zeros(air.modes)) for e in np.eye(na)]
            T=np.column_stack([x[0] for x in columns]);F=np.column_stack([x[1] for x in columns])
            columns=[discrete(np.zeros(na),u) for u in np.eye(air.modes)]
            U=np.column_stack([x[0] for x in columns]);D=np.column_stack([x[1] for x in columns])
            for hz in [1000.,6000.,12000.]:
                theta=2*np.pi*hz*dt
                old=np.linalg.solve(np.exp(1j*theta)*np.eye(na)-T,U*np.exp(1j*theta/2))
                live=D+F@old*np.exp(-1j*theta/2)
                warped=2j/dt*np.tan(theta/2)
                np.testing.assert_allclose(live,impedance(warped),rtol=1e-9,atol=1e-12)


if __name__=='__main__':unittest.main()
