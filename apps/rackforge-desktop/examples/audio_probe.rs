//! Enumerates every audio backend and tries to open each output.
//!
//! `cargo run -p rackforge-desktop --example audio_probe` with the desktop
//! app CLOSED (ASIO drivers admit one client at a time).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() {
    for host_id in cpal::available_hosts() {
        println!("== host: {}", host_id.name());
        let host = match cpal::host_from_id(host_id) {
            Ok(host) => host,
            Err(error) => {
                println!("   backend unavailable: {error}");
                continue;
            }
        };
        let devices = match host.output_devices() {
            Ok(devices) => devices,
            Err(error) => {
                println!("   output_devices() failed: {error}");
                continue;
            }
        };
        for device in devices {
            let name = device.name().unwrap_or_else(|_| "<sin nombre>".into());
            println!("   device: {name}");
            match device.default_output_config() {
                Ok(config) => println!(
                    "      default config: {} ch, {} Hz, {:?}, buffer {:?}",
                    config.channels(),
                    config.sample_rate().0,
                    config.sample_format(),
                    config.buffer_size()
                ),
                Err(error) => println!("      default_output_config FAILED: {error}"),
            }
            match device.supported_output_configs() {
                Ok(configs) => {
                    for config in configs {
                        println!(
                            "      supported: {} ch, {}-{} Hz, {:?}",
                            config.channels(),
                            config.min_sample_rate().0,
                            config.max_sample_rate().0,
                            config.sample_format()
                        );
                    }
                }
                Err(error) => println!("      supported_output_configs FAILED: {error}"),
            }
            // Try to actually open and briefly run a silent stream.
            let Ok(config) = device.default_output_config() else {
                continue;
            };
            let stream_config: cpal::StreamConfig = config.config();
            match device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    for sample in data.iter_mut() {
                        *sample = 0.0;
                    }
                },
                |error| eprintln!("      stream error: {error}"),
                None,
            ) {
                Ok(stream) => match stream.play() {
                    Ok(()) => {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        println!("      STREAM OK (f32)");
                    }
                    Err(error) => println!("      play() FAILED: {error}"),
                },
                Err(error) => println!("      build_output_stream FAILED: {error}"),
            }
        }
    }
}
