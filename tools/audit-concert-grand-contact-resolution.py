"""Modal sensitivity and unpaid initialization work of the SI contact pilot."""
import argparse,json,time
from pathlib import Path
import numpy as np
from concert_grand_nonlinear_string import String
from concert_grand_coupled_contact import HammerString
from importlib.util import spec_from_file_location,module_from_spec

spec=spec_from_file_location('contact_audit',Path(__file__).with_name('audit-concert-grand-coupled-contact.py'))
audit=module_from_spec(spec);spec.loader.exec_module(audit)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True);report=dict(native_linear_handoff={},axial_resolution={})
    for note in (60,72):
        materials=[]
        for velocity in (60,124):
            system,speed,rows=audit.load_case(args.input,note,velocity);core=system.string
            materials.append((core.wire.length,core.wire.linear_density,core.wire.tension))
            q=np.zeros(core.size);p=np.zeros(core.size)
            for row in rows:
                h=int(row['harmonic'])-1
                if h>=core.nt:continue
                q[core.nl+h]=np.sqrt(core.wire.modal_mass)*float(row['q_m'])
                p[core.nl+h]=np.sqrt(core.wire.modal_mass)*float(row['velocity_over_omega_m'])*2*np.pi*float(row['frequency_hz'])
            linear=.5*float(p@p)+.5*float(q@core.gradient_hessian(np.zeros(core.size))[1]@q)
            nonlinear=core.energy(q,p)
            report['native_linear_handoff'][f'{note}-{velocity}']=dict(transverse_modes_used=core.nt,axial_initial_state='zero',
                linear_energy_j=linear,unpaid_extra_stretch_j=nonlinear-linear,unpaid_fraction=(nonlinear-linear)/linear,
                caveat='Diagnostic on the first 12 native modes, not the full native contact or a measured piano')
        assert np.array_equal(materials[0],materials[1]),'material must not depend on velocity'
        report['axial_resolution'][str(note)]={}
        for nl in (2,4,8):
            system,speed,_=audit.load_case(args.input,note,124)
            core=String(system.string.wire,transverse=12,axial=nl,quadrature=192,bending=False)
            system=HammerString(core,system.felt)
            ref,info=audit.reference(system,speed,.006);n=system.size;state=ref.y_events[0][0]
            q,p=state[:n],state[n:2*n];components=core.components(q[:-1],p[:-1])
            info.update(string_energy_at_separation_j=core.energy(q[:-1],p[:-1]),components_at_separation=components,
                        final_components=core.components(ref.y[:n-1,-1],ref.y[n:2*n-1,-1]))
            assert info['final_balance_relative_error']<1e-8
            report['axial_resolution'][str(note)][str(nl)]=info
            (args.output/'resolution.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(note,'axial modes',nl,'energy',info['string_energy_at_separation_j'],flush=True)


if __name__=='__main__':main()
