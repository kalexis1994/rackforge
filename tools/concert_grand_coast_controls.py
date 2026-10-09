"""Diagnostic coast controls; neither ablation selects piano physics.

Quadratic strings retain mass, stiffness, ports, heads and all coordinates.
Mechanical-only omits the entire air/material subflow and freezes air states.
Air subcycling changes only the time partition inside the passive subflow.
"""
import numpy as np
from concert_grand_shared_strings import SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast


def quadratic_system(reference):
    core=BatchedStrings(reference.string)
    core.nonlinear=False
    result=SharedHammers(core,reference.felt)
    if core.K is not reference.string.K or core.board_transform is not reference.string.board_transform:
        raise ValueError('unchanged quadratic matrices required')
    return result


def quadratic_energy(pilot,state):
    if pilot.system.string.nonlinear: raise ValueError('quadratic control required')
    n=pilot.system.string.size; q,v=state[:2]
    omega=np.sqrt(pilot.oscillator_functions(1/128000)[3])
    modes=omega*pilot.project(q[:n])
    return .5*float(modes@modes+v@v)


class MechanicalOnlyCoast(PackedCoast):
    def step(self,state,dt,defer_radiation=False):
        if self.system.string.nonlinear: raise ValueError('quadratic mechanical control required')
        q,v,stats=self.mechanical_step(state[0],state[1],dt)
        stats.update(felt_loss_j=0.,material_loss_j=0.,radiation_loss_j=0.,
                     air_port_work_j=0.,loss_j=0.,force_n=0.)
        return (q,v,state[2],state[3]),stats


class AirSubcycledCoast(PackedCoast):
    def __init__(self,system,air,residues=None,substeps=8):
        if int(substeps)!=substeps or substeps<1: raise ValueError('positive integer air substeps')
        self.substeps=int(substeps)
        super().__init__(system,air,residues)

    def air_step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0: raise ValueError('positive air interval')
        result=state
        total={k:0. for k in ['felt_loss_j','material_loss_j','radiation_loss_j','air_port_work_j','loss_j','force_n']}
        packets=[]
        for _ in range(self.substeps):
            result,stats=super().air_step(result,dt/self.substeps,defer_radiation)
            for key in total: total[key]+=stats[key]
            if defer_radiation: packets.append(stats['wave_inputs'])
        if defer_radiation:
            # Keep the actual microstep packets, never treat their sum as
            # one radiation sample. This diagnostic is not the WAV renderer.
            total['wave_inputs_substeps']=np.asarray(packets)
        return result,total


class WorkMeter:
    def __init__(self,pilot):
        self.pilot=pilot; self.system=pilot.system
        self.air_work_j=0.; self.material_heat_j=0.

    def step(self,state,dt,defer_radiation=False):
        result,stats=self.pilot.step(state,dt,defer_radiation)
        self.air_work_j+=stats['air_port_work_j']
        self.material_heat_j+=stats['material_loss_j']
        return result,stats
