"""Measure SI contact -> empirical state mapping and absolute pole reference.

Reads frozen native CSVs; does not change the plugin or infer real Steinway
displacements from audio. Young/density are explicit plain-steel assumptions.
Quasistatic mean-tension estimates are observers, not energy-safe candidates.
"""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import numpy as np
from concert_grand_physical_coordinates import (
    mass_normalized,linear_energy_j,tension_force_projection,
    mean_excess_length_fraction,verify_coordinates)


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--transfer',type=Path,required=True)
    p.add_argument('--precision',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--binary',type=Path)
    p.add_argument('--work',type=Path)
    p.add_argument('--expected-sha256')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    if a.binary:
        assert a.work and sha(a.binary)==a.expected_sha256
        transfer_files=[];precision_files=[];end_files=[]
        for note in (42,60,72,84,90):
            for velocity in (60,124):
                folder=a.work/'transfer'/f'{note}-{velocity}';folder.mkdir(parents=True,exist_ok=True)
                env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
                env.update(CG_P3_TRANSFER_DIR=str(folder.resolve()),CG_P3_PROBE_NOTE=str(note),
                    CG_P3_PROBE_VELOCITY=str(velocity),RUST_MIN_STACK='33554432')
                with (folder/'test.log').open('w') as log:
                    subprocess.run([str(a.binary.resolve()),'nonlinear_wires::tests::export_contact_state_transfer',
                        '--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
                transfer_files.append(folder/'transfer.csv')
                end_files.append(folder/'contact-end.csv')
            for mode in (0,1,2,3):
                for reference in (0,1):
                    folder=a.work/'precision'/f'{note}-{mode}-{reference}';folder.mkdir(parents=True,exist_ok=True)
                    env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
                    env.update(CG_P3_PRECISION_DIR=str(folder.resolve()),CG_P3_PROBE_NOTE=str(note),
                        CG_P3_PROBE_MODE=str(mode),CG_P3_PROBE_REFERENCE=str(reference),RUST_MIN_STACK='33554432')
                    with (folder/'test.log').open('w') as log:
                        subprocess.run([str(a.binary.resolve()),'nonlinear_wires::tests::export_tension_reference_precision',
                            '--exact','--ignored','--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
                    precision_files.append(folder/'precision.csv')
        for target,files in ((a.transfer,transfer_files),(a.precision,precision_files),
                             (a.transfer.parent/'contact-end.csv',end_files)):
            target.parent.mkdir(parents=True,exist_ok=True)
            with target.open('w',newline='') as output:
                writer=None
                for path in files:
                    with path.open(newline='') as source:
                        reader=csv.DictReader(source)
                        if writer is None:writer=csv.DictWriter(output,fieldnames=reader.fieldnames);writer.writeheader()
                        assert reader.fieldnames==writer.fieldnames
                        writer.writerows(reader)
    grouped={}
    with a.transfer.open(newline='') as f:
        for row in csv.DictReader(f):
            grouped.setdefault((int(row['note']),int(row['velocity'])),[]).append(row)
    report=dict(policy=__doc__,coordinate_gates=verify_coordinates(),contact={},precision={},
                material_assumptions=dict(young_pa=200e9,density_kg_m3=7850),selection=None,factory_changed=False,
                process_isolation=bool(a.binary),binary_sha256=sha(a.binary) if a.binary else None)
    end_path=a.transfer.parent/'contact-end.csv'
    with end_path.open(newline='') as f:
        endings={f"{r['note']}:{r['velocity']}":r for r in csv.DictReader(f)}
    all_forces=[];all_native=[]
    for (note,velocity),rows in grouped.items():
        # Factory is a representative single-string contact, then split/voiced.
        assert all(int(r['joint'])==0 for r in rows)
        wire=[r for r in rows if int(r['wire'])==0]
        h=np.array([int(r['harmonic']) for r in wire]);q=np.array([float(r['q_m']) for r in wire])
        o=np.array([float(r['velocity_over_omega_m']) for r in wire]);hz=np.array([float(r['frequency_hz']) for r in wire])
        length,tension,mass=(float(wire[0][k]) for k in ('length_m','tension_n','modal_mass_kg'))
        Q,P=mass_normalized(q,o,hz,mass)
        energy=linear_energy_j(q,o,hz,mass)
        assert abs(.5*np.sum(P*P+(2*np.pi*hz*Q)**2)-energy)<1e-12
        end=endings[f'{note}:{velocity}']
        assert abs(float(end['string_energy_j'])-energy)/max(energy,1e-30)<1e-5
        strain=float(mean_excess_length_fraction(q,o,length,h))
        mu=2*mass/length;EA=200e9*mu/7850
        # A hypothetical uniform mean tension after axial relaxation.
        delta=EA*strain
        offset=np.sqrt(1+delta/tension)-1
        native=np.array([np.hypot(float(r['native_s']),float(r['native_c'])) for r in wire])
        forces=abs(tension_force_projection(length,tension,h))*np.hypot(q,o)
        built=np.array([bool(int(r['built'])) for r in wire])
        selected=built&(forces>max(forces)*1e-4)
        gains=native[selected]/forces[selected]
        all_forces.extend(forces[selected]);all_native.extend(native[selected])
        modal_gain=native/np.maximum(np.hypot(q,o),1e-30)
        report['contact'][f'{note}:{velocity}']=dict(modes=len(wire),built_modes=int(built.sum()),
            length_m=length,tension_n=tension,modal_mass_kg=mass,contact_linear_energy_per_representative_wire_j=float(energy),
            head_initial_energy_per_string_j=.5*float(wire[0]['hammer_mass_per_string_kg'])*float(wire[0]['hammer_speed_m_s'])**2,
            mean_excess_length_fraction=strain,
            plain_steel_quasistatic_mean_tension_rise_n=float(delta),
            plain_steel_quasistatic_ideal_pitch_offset_cents=float(1200*np.log2(1+offset)),
            material_interpretation=('wound bass: inferred EA is not a valid core identification' if note==42
                                     else 'assumed solid steel, not an identified real string'),
            leading_tension_force_to_native_median_gain=float(np.median(gains)),
            leading_tension_force_to_native_gain_range=[float(gains.min()),float(gains.max())],
            fundamental_native_units_per_meter=float(modal_gain[0]),
            first_contact_frequency_hz=float(hz[0]),
            termination={k:(bool(int(v)) if k=='cap_reached' else float(v)) for k,v in end.items() if k not in ('note','velocity')},
            harmonic=h.tolist(),native_units_per_meter=modal_gain.tolist())
    f=np.array(all_forces);n=np.array(all_native)
    gain=float(f@n/(f@f))
    report['one_global_force_gain_control']=dict(gain_native_per_n=gain,
        relative_rms_mapping_error=float(np.sqrt(np.mean((gain*f-n)**2)/np.mean(n*n))),
        warning='Diagnostic leading-tension force, no bending shear; not an audio fit')
    report['velocity_gain_ratios']={str(note):report['contact'][f'{note}:124']['fundamental_native_units_per_meter']/
        report['contact'][f'{note}:60']['fundamental_native_units_per_meter'] for note in sorted({n for n,v in grouped})}
    report['velocity_mass_and_initial_pitch_changes']={str(note):dict(
        contact_modal_mass_ratio=report['contact'][f'{note}:124']['modal_mass_kg']/report['contact'][f'{note}:60']['modal_mass_kg'],
        first_frequency_change_cents=float(1200*np.log2(report['contact'][f'{note}:124']['first_contact_frequency_hz']/
            report['contact'][f'{note}:60']['first_contact_frequency_hz']))) for note in sorted({n for n,v in grouped})}
    precision={}
    with a.precision.open(newline='') as f:
        for row in csv.DictReader(f):
            key=f"{row['note']}:{row['mode']}:{row['reference']}"
            precision.setdefault(key,[]).append(row)
    for key,rows in precision.items():
        report['precision'][key]=dict(maximum_requested_offset=max(abs(float(r[f'requested{i}'])) for r in rows for i in range(3)),
            maximum_fundamental_pole_shift_cents=max(abs(float(r[f'pole_shift{i}_cents'])) for r in rows for i in range(3)),
            maximum_physical_vertical_pole_shift_cents=max(float(r['maximum_physical_shift_cents']) for r in rows),
            warning='First 120 ms, velocity124; stored poles, not complete audio pitch')
    report['sha256']={str(path):sha(path) for path in [a.transfer,a.precision,end_path,Path(__file__),
        Path(__file__).with_name('concert_grand_physical_coordinates.py')]}
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(dict(coordinate_gates=report['coordinate_gates'],
        contact={k:{n:v for n,v in r.items() if n not in ('harmonic','native_units_per_meter')} for k,r in report['contact'].items()},
        velocity_gain_ratios=report['velocity_gain_ratios'],one_global_force_gain_control=report['one_global_force_gain_control']),indent=2))


if __name__=='__main__':main()
