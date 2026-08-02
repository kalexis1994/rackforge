#![cfg_attr(target_arch = "wasm32", no_std)]

use rackforge_plugin_sdk::{MidiEvent, ParameterEvent, Processor, export_processor};

struct Gain {
    level: f32,
}

impl Default for Gain {
    fn default() -> Self {
        Self { level: 1.0 }
    }
}

impl Processor for Gain {
    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        if index != 0 || !(0.0..=2.0).contains(&value) {
            return false;
        }
        self.level = value as f32;
        true
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        (index == 0).then_some(self.level as f64)
    }

    fn load_preset(&mut self, id: &str) -> bool {
        self.level = match id {
            "mute" => 0.0,
            "half" => 0.5,
            "unity" => 1.0,
            _ => return false,
        };
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        let target = destination.get_mut(..4)?;
        target.copy_from_slice(&self.level.to_le_bytes());
        Some(4)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        let Ok(bytes) = <[u8; 4]>::try_from(state) else {
            return false;
        };
        let level = f32::from_le_bytes(bytes);
        if !level.is_finite() || !(0.0..=2.0).contains(&level) {
            return false;
        }
        self.level = level;
        true
    }

    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        _midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) {
        let input_channels = input_channels as usize;
        let output_channels = output_channels as usize;
        let mut parameter_index = 0;

        for frame in 0..frames as usize {
            while let Some(event) = parameters.get(parameter_index) {
                if event.frame as usize != frame {
                    break;
                }
                let _ = self.set_parameter(event.index, event.value);
                parameter_index += 1;
            }

            for channel in 0..output_channels {
                let input_sample = if channel < input_channels {
                    input[frame * input_channels + channel]
                } else {
                    0.0
                };
                output[frame * output_channels + channel] = input_sample * self.level;
            }
        }
    }
}

export_processor!(
    Gain,
    max_frames = 4096,
    max_input_channels = 2,
    max_output_channels = 2,
    max_midi_events = 256,
    max_parameter_events = 256,
    max_transfer_bytes = 4096
);

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_is_platform_independent() {
        let mut gain = Gain::default();
        assert!(gain.set_parameter(0, 0.25));
        let mut output = [0.0; 2];
        gain.process(&[1.0, -1.0], &mut output, &[], &[], 1, 2, 2);
        assert_eq!(output, [0.25, -0.25]);
    }

    #[test]
    fn applies_parameter_events_at_the_requested_frame() {
        let mut gain = Gain::default();
        let input = [1.0, 1.0, 1.0, 1.0];
        let mut output = [0.0; 4];
        gain.process(
            &input,
            &mut output,
            &[],
            &[ParameterEvent {
                frame: 1,
                index: 0,
                value: 0.5,
            }],
            2,
            2,
            2,
        );
        assert_eq!(output, [1.0, 1.0, 0.5, 0.5]);
    }

    #[test]
    fn preset_and_opaque_state_round_trip() {
        let mut gain = Gain::default();
        assert!(gain.load_preset("half"));
        let mut state = [0; 8];
        assert_eq!(gain.save_state(&mut state), Some(4));
        assert!(gain.set_parameter(0, 2.0));
        assert!(gain.load_state(&state[..4]));
        assert_eq!(gain.level, 0.5);
    }
}
