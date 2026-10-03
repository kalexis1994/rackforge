//! Native P3 attribution control, not an energy-conserving physical engine.
//! Phasor amplitudes, slope h/16 and tension gain retain empirical units.
//! Per-wire geometric statistics remove false inter-wire/polarization products.
//! Phase averaging diagnoses control-rate aliasing, not the full audio-rate
//! nonlinear exchange of the separate SI Hamiltonian pilot.
use crate::{math::sqrtf, polarization::HORIZONTAL_LANES, Partial, Voice,
            TENSION_MAX_SHIFT, TENSION_SMOOTHING, TENSION_REFERENCE};

/// Absolute angle in f64; f32 coefficients retain their final quantization.
/// Radius is cached to avoid repeated re-quantization creating damping drift.
/// External damper changes update the radius without resetting the frequency.
#[derive(Clone, Copy, Default)]
pub struct ReferencePole {
    angle: f64,
    radius: f64,
    last_rc: f32,
    last_rs: f32,
    valid: bool,
}

pub fn reference_shift(partial: &mut Partial, lane: usize, offset: f64) {
    let rc=partial.rc[lane] as f64;
    let rs=partial.rs[lane] as f64;
    let state=&mut partial.tension_reference[lane];
    if rc==0.0 && rs==0.0 { *state=ReferencePole::default();return; }
    if !state.valid {
        state.angle=rs.atan2(rc);
        state.radius=rc.hypot(rs);
        state.valid=true;
    } else if partial.rc[lane]!=state.last_rc || partial.rs[lane]!=state.last_rs {
        // The native experiment assumes intervening edits only scale damping.
        // Live geometry/voicing switches or another phase editor are unsupported.
        state.radius=rc.hypot(rs);
    }
    let (sin,cos)=(state.angle*(1.0+offset)).sin_cos();
    partial.rc[lane]=(state.radius*cos) as f32;
    partial.rs[lane]=(state.radius*sin) as f32;
    state.last_rc=partial.rc[lane];
    state.last_rs=partial.rs[lane];
}

use std::sync::{Mutex,atomic::{AtomicBool,Ordering}};
static TRANSFER_ARMED:AtomicBool=AtomicBool::new(false);
#[derive(Clone)]
struct TransferRow {
    note:u8, harmonic:usize, wire:usize, frequency:f64, length:f64,
    tension:f64, modal_mass:f64, hammer_mass:f64, speed:f64, contact_cap:f64,
    q:f64, over_omega:f64, native_s:f64, native_c:f64, built:bool, joint:bool,
}
static TRANSFER:Mutex<Vec<TransferRow>>=Mutex::new(Vec::new());

pub fn capture_contact(note:u8,length:f32,tension:f32,c:crate::StrikeConfiguration,
    frequencies:&[f32],q:&[f32],o:&[f32],trio:Option<&crate::trio_contact::Strike>) {
    if !TRANSFER_ARMED.load(Ordering::Relaxed) {return;}
    let mut rows=TRANSFER.lock().unwrap();
    for (n,f) in frequencies.iter().enumerate() {
        for wire in 0..3 {
            let (q,o)=if let Some(r)=trio {(r.q[wire][n],r.over_omega[wire][n])} else {(q[n],o[n])};
            rows.push(TransferRow{note,harmonic:n+1,wire,frequency:*f as f64,
                length:length as f64,tension:tension as f64,modal_mass:c.string_mass as f64/2.0,
                hammer_mass:c.mass as f64,speed:c.velocity as f64,contact_cap:c.contact_seconds as f64,
                q:q as f64,over_omega:o as f64,
                native_s:0.0,native_c:0.0,built:false,joint:trio.is_some()});
        }
    }
}

pub fn capture_built(note:u8,harmonic:usize,partial:&Partial) {
    if !TRANSFER_ARMED.load(Ordering::Relaxed) {return;}
    for row in TRANSFER.lock().unwrap().iter_mut().filter(|r|r.note==note&&r.harmonic==harmonic) {
        row.native_s=partial.s[row.wire] as f64;
        row.native_c=partial.c[row.wire] as f64;
        row.built=true;
    }
}

pub fn stretch<const P: usize>(voice: &Voice<P>, averaged: bool) -> [f32; 3] {
    let mut total = [0.0; 3];
    for partial in &voice.partials[..voice.partial_count] {
        if partial.slope == 0.0 { continue; }
        assert!(partial.polarization_mask != 0, "P3 geometry needs individual H states");
        let weight = partial.slope * partial.slope;
        for (wire, horizontal) in HORIZONTAL_LANES.iter().enumerate() {
            if partial.polarization_mask & (1 << wire) == 0 { continue; }
            let statistic = if averaged {
                0.5 * (partial.lane_magnitude_squared(wire)
                    + partial.lane_magnitude_squared(*horizontal))
            } else {
                partial.s[wire]*partial.s[wire]
                    + partial.s[*horizontal]*partial.s[*horizontal]
            };
            total[wire] += weight * statistic;
        }
    }
    total
}

pub fn tension_step<const P: usize>(voice: &mut Voice<P>, averaged: bool) {
    let geometric = stretch(voice, averaged);
    let limit = TENSION_MAX_SHIFT.get();
    let smoothing = TENSION_SMOOTHING.get();
    let mut rates = [0.0; 3];
    for wire in 0..3 {
        let desired = (voice.tension_gain * geometric[wire]).clamp(0.0, limit);
        voice.wire_tension_smoothed[wire] +=
            (desired - voice.wire_tension_smoothed[wire]) * smoothing;
        rates[wire] = voice.wire_tension_smoothed[wire] - voice.wire_tension_applied[wire];
        voice.wire_tension_applied[wire] = voice.wire_tension_smoothed[wire];
    }
    for partial in &mut voice.partials[..voice.partial_count] {
        if partial.slope == 0.0 { continue; }
        for (wire, horizontal) in HORIZONTAL_LANES.iter().enumerate() {
            if partial.polarization_mask & (1 << wire) == 0 || rates[wire] == 0.0 { continue; }
            for lane in [wire, *horizontal] {
                if TENSION_REFERENCE.get()>0.0 {
                    reference_shift(partial,lane,voice.wire_tension_smoothed[wire] as f64);
                    continue;
                }
                // Retain the old radius-preserving phase nudge. It preserves
                // phasor norm, which is not proof of conservation in joules.
                let step = rates[wire] * partial.rs[lane];
                let scale = 1.0 / sqrtf(1.0 + step * step);
                let rc = (partial.rc[lane] - partial.rs[lane] * step) * scale;
                partial.rs[lane] = (partial.rs[lane] + partial.rc[lane] * step) * scale;
                partial.rc[lane] = rc;
            }
        }
    }
}

pub fn add_end_slopes(slopes: &mut [[f32; 2]; 3], partial: &Partial) {
    assert!(partial.polarization_mask != 0);
    for (wire, output) in slopes.iter_mut().enumerate() {
        if partial.polarization_mask & (1 << wire) != 0 {
            output[0] += partial.slope * partial.geometry_out[wire];
            output[1] += partial.slope * partial.geometry_out[wire + 3];
        }
    }
}

pub fn end_force_statistic(slopes: &[[f32; 2]; 3]) -> f32 {
    slopes.iter().map(|s| s[0]*s[0]+s[1]*s[1]).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "offline P3 requested tension versus representable pole updates"]
    fn export_native_tension_precision() {
        export_precision(false);
    }
    #[test]
    #[ignore = "offline P3 absolute-reference tension precision comparison"]
    fn export_tension_reference_precision() {
        export_precision(true);
    }
    fn export_precision(compare_reference:bool) {
        use std::io::Write;
        use crate::{ConcertGrand, MidiEvent, NONLINEAR_WIRE_GEOMETRY, PER_STRING_POLARIZATIONS};
        let folder=std::env::var("CG_P3_PRECISION_DIR").expect("CG_P3_PRECISION_DIR");
        std::fs::create_dir_all(&folder).unwrap();
        let mut file=std::io::BufWriter::new(std::fs::File::create(format!("{folder}/precision.csv")).unwrap());
        writeln!(file,"note,mode,time_s,requested0,requested1,requested2,pole_shift0_cents,pole_shift1_cents,pole_shift2_cents,changed_poles,physical_changed_poles,maximum_physical_shift_cents,reference").unwrap();
        for note in [42u8,60,72,84,90] {
            if let Ok(only)=std::env::var("CG_P3_PROBE_NOTE") {if note!=only.parse::<u8>().unwrap(){continue;}}
            for mode in 0..=3 {
              if let Ok(only)=std::env::var("CG_P3_PROBE_MODE") {if mode!=only.parse::<usize>().unwrap(){continue;}}
              for reference in 0..=usize::from(compare_reference) {
                if let Ok(only)=std::env::var("CG_P3_PROBE_REFERENCE") {if reference!=only.parse::<usize>().unwrap(){continue;}}
                crate::bench::reset_every_knob();
                TENSION_REFERENCE.set(reference as f32);
                PER_STRING_POLARIZATIONS.set(if mode==0 {0.0} else {1.0});
                NONLINEAR_WIRE_GEOMETRY.set(mode as f32);
                let mut piano=Box::new(ConcertGrand::default());
                assert!(piano.load_preset("concert"));
                assert!(piano.set_parameter(25,2.64));assert!(piano.set_parameter(31,4.6));
                assert!(piano.prepare(44100.0,256,0,2));
                let mut output=[0.0f32;512];
                for _ in 0..35 {piano.process(&[],&mut output,&[],&[],256,0,2);}
                let midi=MidiEvent{frame:0,data:[0x90,note,124],length:3};
                piano.process(&[],&mut output,&[midi],&[],1,0,2);
                let fundamental=|voice:&crate::Voice|voice.partials[..voice.partial_count]
                    .iter().find(|p|p.slope==1.0/16.0).copied().expect("fundamental");
                let first=fundamental(piano.voices().find(|v|v.active&&v.note==note).unwrap());
                let first_voice=piano.voices().find(|v|v.active&&v.note==note).unwrap();
                let first_all=first_voice.partials[..first_voice.partial_count].to_vec();
                let mut previous_all=first_all.clone();
                let mut previous=first;
                for step in 0..165 {
                    piano.process(&[],&mut output,&[],&[],32,0,2);
                    let voice=piano.voices().find(|v|v.active&&v.note==note).unwrap();
                    let now=fundamental(voice);
                    let shifts: [f64;3]=std::array::from_fn(|i|{
                        let a=(first.rs[i] as f64).atan2(first.rc[i] as f64);
                        let b=(now.rs[i] as f64).atan2(now.rc[i] as f64);
                        1200.0*(b/a).log2()
                    });
                    let changed=(0..3).filter(|i|now.rc[*i]!=previous.rc[*i]||now.rs[*i]!=previous.rs[*i]).count();
                    previous=now;
                    let mut physical_changed=0;
                    let mut maximum_physical_shift=0.0f64;
                    for (index,p) in voice.partials[..voice.partial_count].iter().enumerate() {
                        if p.slope==0.0 {continue;}
                        for wire in 0..3 {
                            if p.rs[wire]==0.0 {continue;}
                            physical_changed+=usize::from(p.rc[wire]!=previous_all[index].rc[wire]
                                ||p.rs[wire]!=previous_all[index].rs[wire]);
                            let a=(first_all[index].rs[wire] as f64).atan2(first_all[index].rc[wire] as f64);
                            let b=(p.rs[wire] as f64).atan2(p.rc[wire] as f64);
                            maximum_physical_shift=maximum_physical_shift.max((1200.0*(b/a).log2()).abs());
                        }
                        previous_all[index]=*p;
                    }
                    let requested=if mode==0 {[voice.tension_applied;3]} else {voice.wire_tension_applied};
                    writeln!(file,"{},{},{},{},{},{},{},{},{},{},{},{},{}",note,mode,(1+32*(step+1)) as f64/44100.0,
                        requested[0],requested[1],requested[2],shifts[0],shifts[1],shifts[2],changed,
                        physical_changed,maximum_physical_shift,reference).unwrap();
                }
              }
            }
        }
        crate::bench::reset_every_knob();
    }
    #[test]
    #[ignore = "offline P3 SI contact versus constructed empirical states"]
    fn export_contact_state_transfer() {
        use crate::{ConcertGrand,MidiEvent};
        use std::io::Write;
        let folder=std::env::var("CG_P3_TRANSFER_DIR").expect("CG_P3_TRANSFER_DIR");
        std::fs::create_dir_all(&folder).unwrap();
        let mut file=std::io::BufWriter::new(std::fs::File::create(format!("{folder}/transfer.csv")).unwrap());
        writeln!(file,"note,velocity,harmonic,wire,frequency_hz,length_m,tension_n,modal_mass_kg,hammer_mass_per_string_kg,hammer_speed_m_s,q_m,velocity_over_omega_m,native_s,native_c,built,joint").unwrap();
        let mut end_file=std::io::BufWriter::new(std::fs::File::create(format!("{folder}/contact-end.csv")).unwrap());
        writeln!(end_file,"note,velocity,dt_s,processed_steps,positive_compression_steps,cap_steps,cap_reached,contact_time_s,string_energy_j,initial_hammer_energy_j,final_hammer_energy_j,stiffness_n_per_m_p,exponent,width_fraction,strike_fraction,final_hammer_speed_m_s").unwrap();
        for note in [42u8,60,72,84,90] {for velocity in [60u8,124] {
            if let Ok(only)=std::env::var("CG_P3_PROBE_NOTE") {if note!=only.parse::<u8>().unwrap(){continue;}}
            if let Ok(only)=std::env::var("CG_P3_PROBE_VELOCITY") {if velocity!=only.parse::<u8>().unwrap(){continue;}}
            crate::bench::reset_every_knob();
            let mut piano=Box::new(ConcertGrand::default());
            assert!(piano.load_preset("concert"));
            assert!(piano.set_parameter(25,2.64));assert!(piano.set_parameter(31,4.6));
            assert!(piano.prepare(44100.0,256,0,2));
            let mut output=[0.0f32;512];
            for _ in 0..35 {piano.process(&[],&mut output,&[],&[],256,0,2);}
            TRANSFER.lock().unwrap().clear();
            crate::CONTACT_TRACE.lock().unwrap().clear();
            *crate::CONTACT_AUDIT.lock().unwrap()=None;
            TRANSFER_ARMED.store(true,Ordering::Relaxed);
            crate::CONTACT_TRACE_ARMED.store(true,Ordering::Relaxed);
            let midi=MidiEvent{frame:0,data:[0x90,note,velocity],length:3};
            piano.process(&[],&mut output,&[midi],&[],1,0,2);
            TRANSFER_ARMED.store(false,Ordering::Relaxed);
            crate::CONTACT_TRACE_ARMED.store(false,Ordering::Relaxed);
            let rows=TRANSFER.lock().unwrap();
            assert!(!rows.is_empty(),"No simulated contact for {note}");
            let audit=crate::CONTACT_AUDIT.lock().unwrap().expect("contact audit");
            let processed=crate::CONTACT_TRACE.lock().unwrap().len();
            let touched=crate::CONTACT_STEPS.load(Ordering::Relaxed);
            let cap=((rows[0].contact_cap as f32/audit[11] as f32) as usize).clamp(200,6000);
            writeln!(end_file,"{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",note,velocity,audit[11],processed,touched,cap,
                u8::from(processed==cap),touched as f64*audit[11],audit[8],audit[9],audit[10],audit[4],audit[5],audit[6],
                piano.engine.strike_point(note),audit[3]).unwrap();
            for r in rows.iter() {writeln!(file,"{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                r.note,velocity,r.harmonic,r.wire,r.frequency,r.length,r.tension,r.modal_mass,
                r.hammer_mass,r.speed,r.q,r.over_omega,r.native_s,r.native_c,r.built as u8,r.joint as u8).unwrap();}
        }}
        crate::bench::reset_every_knob();
    }
    #[test]
    fn absolute_reference_retains_sub_ulp_increments_and_returns_to_base() {
        for angle in [0.002f64,0.037,0.35,1.4] {
            let mut p=Partial::default();let mut incremental=Partial::default();
            p.rc[0]=(0.9999*angle.cos()) as f32;p.rs[0]=(0.9999*angle.sin()) as f32;
            incremental.rc[0]=p.rc[0];incremental.rs[0]=p.rs[0];
            let base=(p.rs[0] as f64).atan2(p.rc[0] as f64);
            let radius=(p.rc[0] as f64).hypot(p.rs[0] as f64);
            let original=(p.rc[0],p.rs[0]);
            for i in 0..20000 {
                reference_shift(&mut p,0,(i+1) as f64*1e-9);
                let step=1e-9f32*incremental.rs[0];
                incremental.rc[0]-=incremental.rs[0]*step;
                incremental.rs[0]+=incremental.rc[0]*step;
            }
            assert_eq!((incremental.rc[0],incremental.rs[0]),original);
            let expected=base*(1.0+2e-5);
            let now=(p.rs[0] as f64).atan2(p.rc[0] as f64);
            assert!((now-expected).abs()<8e-8*base,"{angle} {now} {expected}");
            assert!(((p.rc[0] as f64).hypot(p.rs[0] as f64)-radius).abs()<5e-8);
            for i in (0..20000).rev() {reference_shift(&mut p,0,i as f64*1e-9);}
            assert_eq!((p.rc[0],p.rs[0]),original);
        }
    }
    #[test]
    fn reference_retains_external_damper_radius_and_does_not_restart_retired_lanes() {
        let mut p=Partial::default();p.rc[0]=0.8;p.rs[0]=0.5;
        reference_shift(&mut p,0,0.001);
        p.rc[0]*=0.91;p.rs[0]*=0.91;
        let damped=(p.rc[0] as f64).hypot(p.rs[0] as f64);
        reference_shift(&mut p,0,0.0007);
        assert!(((p.rc[0] as f64).hypot(p.rs[0] as f64)-damped).abs()<5e-8);
        p.rc[0]=0.0;p.rs[0]=0.0;reference_shift(&mut p,0,0.001);
        assert_eq!((p.rc[0],p.rs[0]),(0.0,0.0));
        assert!(!p.tension_reference[0].valid);
    }
    #[test]
    fn another_wire_cannot_cancel_or_create_the_first_wires_stretch() {
        let mut voice = Box::<Voice>::new(Voice::default());
        voice.partial_count=1;
        let partial=&mut voice.partials[0];
        partial.slope=1.0;partial.polarization_mask=3;
        partial.s[0]=1.0;partial.s[1]=-1.0;
        assert_eq!(stretch(&voice,false),[1.0,1.0,0.0]);
        voice.partials[0].s[1]=1.0;
        assert_eq!(stretch(&voice,false),[1.0,1.0,0.0]);
        voice.partials[0].s[5]=2.0;
        assert_eq!(stretch(&voice,false),[1.0,5.0,0.0]);
    }
    #[test]
    fn phase_average_excludes_control_rate_phase_and_auxiliary_states() {
        let mut voice=Box::<Voice>::new(Voice::default());
        voice.partial_count=2;
        let p=&mut voice.partials[0];
        p.polarization_mask=1;p.slope=0.5;p.s[0]=3.0;p.c[0]=4.0;
        p.s[3]=-4.0;p.c[3]=3.0;p.s[4]=999.0;
        let a=stretch(&voice,true);
        assert_eq!(a,[6.25,0.0,0.0]);
        voice.partials[0].s.swap(0,3);voice.partials[0].c.swap(0,3);
        voice.partials[1].s[0]=999.0; // slope zero: not a speaking mode.
        assert_eq!(stretch(&voice,true),a);
    }
    #[test]
    fn end_force_keeps_pairs_inside_a_wire_but_excludes_cross_wire_pairs() {
        let mut p=Partial::default();p.polarization_mask=3;p.slope=1.0;
        p.geometry_out=[1.0,-1.0,0.0,2.0,-2.0,0.0];
        let mut slopes=[[0.0;2];3];
        add_end_slopes(&mut slopes,&p);
        assert_eq!(end_force_statistic(&slopes),10.0);
        p.geometry_out=[2.0,2.0,0.0,0.0,0.0,0.0];p.slope=-2.0;
        add_end_slopes(&mut slopes,&p);
        assert_eq!(end_force_statistic(&slopes),42.0);
    }
    #[test]
    fn per_wire_nudges_preserve_pole_radii_and_ignore_other_wire_tension() {
        let mut voice=Box::<Voice>::new(Voice::default());voice.partial_count=1;voice.tension_gain=0.052;
        let p=&mut voice.partials[0];p.slope=1.0;p.polarization_mask=3;
        p.s[0]=1.0;p.s[1]=0.0;
        for lane in [0,1,3,5] { p.rc[lane]=0.8;p.rs[lane]=0.5; }
        let old_second=(p.rc[1],p.rs[1],p.rc[5],p.rs[5]);
        let initial=p.rc[0]*p.rc[0]+p.rs[0]*p.rs[0];
        tension_step(&mut voice,false);
        let p=&voice.partials[0];
        assert!((p.rc[0]*p.rc[0]+p.rs[0]*p.rs[0]-initial).abs()<2e-6);
        assert_eq!((p.rc[1],p.rs[1],p.rc[5],p.rs[5]),old_second);
        assert!(voice.wire_tension_applied[0]>0.0);
        assert_eq!(voice.wire_tension_applied[1],0.0);
    }
}
