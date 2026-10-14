//! Compile-time diagnostic clocks; absent from normal processing builds.
#[derive(Clone, Copy)]
pub(super) enum Stage {
    StepSetup,
    NodeSetup,
    Forces,
    NodeUpdate,
    Residual,
    Endpoint,
    RawEnergy,
    Projection,
    Publish,
    Ledger,
}

pub(super) struct Stamp {
    #[cfg(concert_phase_profile)]
    time: std::time::Instant,
}

#[inline(always)]
pub(super) fn start() -> Stamp {
    Stamp {
        #[cfg(concert_phase_profile)]
        time: std::time::Instant::now(),
    }
}

#[derive(Default)]
pub(super) struct Profile {
    #[cfg(concert_phase_profile)]
    seconds: [f64; 10],
    #[cfg(concert_phase_profile)]
    calls: [usize; 10],
}
impl Profile {
    #[inline(always)]
    pub(super) fn record(&mut self, stage: Stage, stamp: Stamp) {
        #[cfg(concert_phase_profile)]
        {
            self.seconds[stage as usize] += stamp.time.elapsed().as_secs_f64();
            self.calls[stage as usize] += 1;
        }
        #[cfg(not(concert_phase_profile))]
        let _ = (stage, stamp);
    }

    pub(super) fn json(&self, processing_seconds: f64) -> String {
        #[cfg(concert_phase_profile)]
        {
            let names = [
                "step_energy_and_linear_setup",
                "node_setup",
                "nonlinear_forces",
                "node_contraction",
                "residual",
                "endpoint",
                "raw_state_and_energy",
                "energy_projection",
                "publish",
                "ledger_monitor",
            ];
            let sections = names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    format!(
                        "{{\"name\":\"{}\",\"milliseconds\":{},\"calls\":{}}}",
                        name,
                        self.seconds[i] * 1000.,
                        self.calls[i]
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let measured = self.seconds.iter().sum::<f64>() * 1000.;
            return format!("{{\"sections\":[{}],\"measured_ms\":{},\"processing_ms\":{},\"unattributed_ms\":{}}}",
                sections, measured, processing_seconds*1000., processing_seconds*1000.-measured);
        }
        #[cfg(not(concert_phase_profile))]
        {
            let _ = processing_seconds;
            "null".to_string()
        }
    }
}
