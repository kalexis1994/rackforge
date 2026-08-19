//! Reproduces the desktop engine's exact stream-opening path against the
//! Focusrite ASIO driver: same config selection, same raw stream with the
//! device's native format. Run with the app closed.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat};

fn main() {
    let target_rates = [44_100u32, 48_000u32];
    let host_id = cpal::available_hosts()
        .into_iter()
        .find(|host| host.name() == "ASIO")
        .expect("ASIO backend missing from this build");
    let host = cpal::host_from_id(host_id).expect("opening ASIO backend");
    let device = host
        .output_devices()
        .expect("enumerating ASIO outputs")
        .find(|device| {
            device
                .name()
                .map(|name| name.contains("Focusrite"))
                .unwrap_or(false)
        })
        .expect("no Focusrite ASIO device");
    println!("device: {}", device.name().unwrap());
    for rate in target_rates {
        let supported = device
            .supported_output_configs()
            .expect("supported configs")
            .filter(|config| {
                config.min_sample_rate().0 <= rate && rate <= config.max_sample_rate().0
            })
            .max_by_key(|config| {
                (
                    config.channels() >= 2,
                    config.sample_format() == SampleFormat::F32,
                    std::cmp::Reverse(config.channels()),
                )
            });
        let Some(supported) = supported else {
            println!("{rate} Hz: no matching config");
            continue;
        };
        let sample_format = supported.sample_format();
        let mut config: cpal::StreamConfig =
            supported.with_sample_rate(cpal::SampleRate(rate)).into();
        config.buffer_size = BufferSize::Default;
        println!(
            "{rate} Hz: trying {} ch, {:?}, buffer Default",
            config.channels, sample_format
        );
        match device.build_output_stream_raw(
            &config,
            sample_format,
            move |data, _| {
                if let Some(out) = data.as_slice_mut::<i32>() {
                    out.fill(0);
                } else if let Some(out) = data.as_slice_mut::<f32>() {
                    out.fill(0.0);
                }
            },
            |error| eprintln!("   stream error: {error}"),
            None,
        ) {
            Ok(stream) => match stream.play() {
                Ok(()) => {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    println!("   RAW STREAM OK");
                    drop(stream);
                }
                Err(error) => println!("   play() FAILED: {error}"),
            },
            Err(error) => println!("   build_output_stream_raw FAILED: {error}"),
        }
    }
}
