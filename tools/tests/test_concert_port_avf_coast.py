import sys, unittest
from pathlib import Path
import numpy as np
from scipy.integrate import solve_ivp
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_port_avf_coast import PortAVFCoast
import test_concert_coast_controls as controls


class PortAVFTests(unittest.TestCase):
    def fixture(self):
        full,old,residues,state=controls.ControlTests().fixture()
        # Nonlinear geometry remains active; every head stays separated.
        for e in full.string.entries: state[0][e['sv'].start]+=1e-6
        return PortAVFCoast(full,old.air,residues),state

    def test_full_energy_loss_and_port_displacement_identity(self):
        pilot,state=self.fixture()
        energy=pilot.energy(state)['total_j']; heat=0.
        for rate in [32000,64000,128000]:
            dt=1/rate; old=state; before=tuple(x.copy() for x in old)
            state,stats=pilot.step(state,dt)
            for x,y in zip(old,before): np.testing.assert_array_equal(x,y)
            heat+=stats['loss_j']
            boundary=pilot.C@((state[0]-old[0])/dt)
            derivative,force=pilot.air.derivative(((state[2]+old[2])/2,(state[3]+old[3])/2),boundary)
            self.assertAlmostEqual(stats['air_port_work_j'],dt*float(boundary@force),places=14)
            for derivative_part,new_part,old_part in zip(derivative,state[2:],old[2:]):
                np.testing.assert_allclose((new_part-old_part)/dt,derivative_part,rtol=2e-10,atol=1e-10)
            self.assertGreaterEqual(stats['material_loss_j'],0.)
            self.assertGreaterEqual(stats['radiation_loss_j'],0.)
            self.assertLess(abs(pilot.energy(state)['total_j']+heat-energy)/energy,1e-9)
        self.assertTrue(pilot.system.string.nonlinear)

    def test_convergence_to_independent_nonlinear_continuous_state_ode(self):
        pilot,initial=self.fixture(); n=pilot.system.size; core=pilot.system.string
        shape=initial[2].shape; wave_size=initial[2].size; duration=.00025
        def unpack(y): return y[:n],y[n:2*n],y[2*n:2*n+wave_size].reshape(shape),y[2*n+wave_size:]
        def rhs(_t,y):
            q,v,r,z=unpack(y)
            gradient=np.r_[core.gradient_hessian(q[:core.size],False),np.zeros(pilot.system.nh)]
            (dr,dz),force=pilot.air.derivative((r,z),pilot.C@v)
            return np.r_[v,-gradient-pilot.system.damping@v-pilot.C.T@force,dr.ravel(),dz]
        y0=np.concatenate([x.ravel() for x in initial])
        ode=solve_ivp(rhs,(0,duration),y0,method='DOP853',rtol=1e-11,atol=1e-15,max_step=1/512000)
        self.assertTrue(ode.success)
        omega=np.sqrt(pilot.oscillator_functions(1/128000)[3])
        def metric(state): return np.r_[omega*pilot.project(state[0][:core.size]),state[1][:core.size],state[2].ravel(),state[3]]
        reference=metric(unpack(ode.y[:,-1])); errors=[]
        for rate in [64000,128000,256000]:
            state=tuple(x.copy() for x in initial)
            for _ in range(round(duration*rate)): state,_=pilot.step(state,1/rate,True)
            errors.append(float(np.linalg.norm(metric(state)-reference)/np.linalg.norm(reference)))
        self.assertTrue(all(errors[i]>3*errors[i+1] for i in [0,1]),errors)
        self.assertLess(errors[-1],.001,errors)


if __name__=='__main__': unittest.main()
