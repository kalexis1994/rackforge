//! A cable's share of the hardware capture, mixed into the plugin it feeds.
//!
//! The engine captures a few physical inputs (the host's audio settings say
//! which) into one interleaved block. Every cable from a Rack's audio input
//! then takes its own inputs from that block -- a guitar on input 1, a voice
//! on input 2 -- at its own trim. This module is that last step, kept free
//! of ALSA so it is tested on every machine and not only on the appliance.

use rackforge_performance_api::RackAudioInputRoute;

/// A cable's route, resolved once when a Rack is activated so the audio
/// thread neither allocates nor converts decibels per block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureRoute {
    /// Physical inputs, one-based; the first `count` are used.
    inputs: [u16; 2],
    /// 0 carries every captured input, as a cable without a route always
    /// has; 1 is a mono source; 2 a stereo pair, left then right.
    count: u8,
    gain: f32,
}

impl CaptureRoute {
    pub fn new(route: &RackAudioInputRoute) -> Self {
        let mut inputs = [0; 2];
        let count = route.channels.len().min(2);
        inputs[..count].copy_from_slice(&route.channels[..count]);
        let gain = 10.0_f32.powf(f32::from(route.gain_db) / 20.0);
        Self {
            inputs,
            count: count as u8,
            gain: if gain.is_finite() { gain } else { 1.0 },
        }
    }

    /// Mixes (adds) this cable's share of one captured block into a
    /// plugin's interleaved input.
    ///
    /// `captured_inputs` names the physical input of each captured channel,
    /// in capture order. An input the host does not capture is silence --
    /// never another input in its place, which would put the voice in the
    /// guitar amp -- and so is a block shorter than it claims to be.
    pub fn mix_into(
        &self,
        capture: &[f32],
        capture_channels: usize,
        captured_inputs: &[u32],
        plugin: &mut [f32],
        plugin_channels: usize,
        frames: usize,
    ) {
        if capture_channels == 0 || plugin_channels == 0 {
            return;
        }
        let frames = frames
            .min(capture.len() / capture_channels)
            .min(plugin.len() / plugin_channels);
        if self.count == 0 {
            self.mix_everything(capture, capture_channels, plugin, plugin_channels, frames);
            return;
        }
        let find = |input: u16| {
            captured_inputs
                .iter()
                .take(capture_channels)
                .position(|captured| *captured == u32::from(input))
        };
        let left = find(self.inputs[0]);
        let right = if self.count == 2 {
            find(self.inputs[1])
        } else {
            left
        };
        if left.is_none() && right.is_none() {
            return;
        }
        let stereo_pair = self.count == 2;
        for frame in 0..frames {
            let captured = &capture[frame * capture_channels..][..capture_channels];
            let l = left.map_or(0.0, |index| captured[index]) * self.gain;
            let r = right.map_or(0.0, |index| captured[index]) * self.gain;
            let out = &mut plugin[frame * plugin_channels..][..plugin_channels];
            if plugin_channels == 1 {
                // A mono plugin takes a mono source as it is and a pair
                // folded to its middle.
                out[0] += if stereo_pair { (l + r) * 0.5 } else { l };
            } else {
                // A mono source reaches both sides; a pair keeps its sides.
                // Any channel past the second carries nothing from here.
                out[0] += l;
                out[1] += r;
            }
        }
    }

    /// No route: everything captured, as it always was -- one input to
    /// every side, two folded for a mono plugin, side for side otherwise.
    fn mix_everything(
        &self,
        capture: &[f32],
        capture_channels: usize,
        plugin: &mut [f32],
        plugin_channels: usize,
        frames: usize,
    ) {
        for frame in 0..frames {
            let captured = &capture[frame * capture_channels..][..capture_channels];
            let out = &mut plugin[frame * plugin_channels..][..plugin_channels];
            for (channel, sample) in out.iter_mut().enumerate() {
                let value = if capture_channels == 1 {
                    captured[0]
                } else if plugin_channels == 1 {
                    (captured[0] + captured[1]) * 0.5
                } else {
                    captured.get(channel).copied().unwrap_or(0.0)
                };
                *sample += value * self.gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(channels: &[u16], gain_db: i8) -> CaptureRoute {
        CaptureRoute::new(&RackAudioInputRoute {
            channels: channels.to_vec(),
            gain_db,
        })
    }

    /// Two frames of inputs 1 and 2 captured: input 1 is 0.1, 0.2; input 2
    /// is 0.5, 0.6.
    const CAPTURE: [f32; 4] = [0.1, 0.5, 0.2, 0.6];
    const INPUTS: [u32; 2] = [1, 2];

    fn mixed(route: CaptureRoute, plugin_channels: usize) -> Vec<f32> {
        let mut plugin = vec![0.0; 2 * plugin_channels];
        route.mix_into(&CAPTURE, 2, &INPUTS, &mut plugin, plugin_channels, 2);
        plugin
    }

    fn assert_close(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 1e-6, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn no_route_carries_everything_captured_as_before() {
        assert_close(&mixed(route(&[], 0), 2), &[0.1, 0.5, 0.2, 0.6]);
        assert_close(&mixed(route(&[], 0), 1), &[0.3, 0.4]);
    }

    #[test]
    fn a_mono_input_reaches_both_sides_of_a_stereo_plugin() {
        assert_close(&mixed(route(&[2], 0), 2), &[0.5, 0.5, 0.6, 0.6]);
        assert_close(&mixed(route(&[1], 0), 1), &[0.1, 0.2]);
    }

    #[test]
    fn a_pair_keeps_its_sides_and_may_be_swapped() {
        assert_close(&mixed(route(&[2, 1], 0), 2), &[0.5, 0.1, 0.6, 0.2]);
        assert_close(&mixed(route(&[1, 2], 0), 1), &[0.3, 0.4]);
    }

    #[test]
    fn the_cable_trim_applies_to_its_share_only() {
        let doubled = mixed(route(&[1], 6), 1);
        assert_close(&doubled, &[0.1 * 1.995_262_3, 0.2 * 1.995_262_3]);
    }

    #[test]
    fn an_input_that_is_not_captured_is_silence_not_another_input() {
        assert_close(&mixed(route(&[3], 0), 2), &[0.0; 4]);
        // Half a pair captured: that side only.
        assert_close(&mixed(route(&[1, 3], 0), 2), &[0.1, 0.0, 0.2, 0.0]);
    }

    #[test]
    fn it_adds_to_what_the_plugin_already_has() {
        let mut plugin = vec![1.0; 4];
        route(&[1], 0).mix_into(&CAPTURE, 2, &INPUTS, &mut plugin, 2, 2);
        assert_close(&plugin, &[1.1, 1.1, 1.2, 1.2]);
    }

    #[test]
    fn a_short_block_is_never_read_past_its_end() {
        let mut plugin = vec![0.0; 8];
        route(&[1], 0).mix_into(&CAPTURE, 2, &INPUTS, &mut plugin, 2, 4);
        assert_close(&plugin, &[0.1, 0.1, 0.2, 0.2, 0.0, 0.0, 0.0, 0.0]);
        let mut short = vec![0.0; 2];
        route(&[], 0).mix_into(&CAPTURE, 2, &INPUTS, &mut short, 2, 2);
        assert_close(&short, &[0.1, 0.5]);
    }

    #[test]
    fn a_mono_capture_feeds_a_stereo_plugin_on_both_sides() {
        let mut plugin = vec![0.0; 4];
        route(&[], 0).mix_into(&[0.3, 0.4], 1, &[2], &mut plugin, 2, 2);
        assert_close(&plugin, &[0.3, 0.3, 0.4, 0.4]);
        let mut routed = vec![0.0; 4];
        route(&[2], 0).mix_into(&[0.3, 0.4], 1, &[2], &mut routed, 2, 2);
        assert_close(&routed, &[0.3, 0.3, 0.4, 0.4]);
    }
}
