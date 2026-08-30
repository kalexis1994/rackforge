use alsa::card::Iter as CardIter;
use alsa::ctl::{Ctl, DeviceIter};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result, bail};
use rackforge_audio_api::{
    AUDIO_DEVICE_SCHEMA_VERSION, AudioBackend, AudioDeviceDescriptor, AudioDeviceId,
    AudioFallbackPolicy, AudioInputProfile, AudioOutputProfile, AudioSampleFormat,
    AudioStreamCapabilities, AudioTransport, AudioValueRange, COMMON_SAMPLE_RATES,
    UsbAudioIdentity,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub struct OpenedAudioOutput {
    pub pcm: PCM,
    pub device: AudioDeviceDescriptor,
    pub profile: AudioOutputProfile,
}

pub struct OpenedAudioInput {
    pub pcm: PCM,
    pub device: AudioDeviceDescriptor,
    pub profile: AudioInputProfile,
}

pub fn discover_audio_devices() -> Result<Vec<AudioDeviceDescriptor>> {
    let mut devices = Vec::new();
    let mut ids = BTreeSet::new();
    for card in CardIter::new() {
        let card = card.context("enumerating ALSA cards")?;
        let index = card.get_index();
        let control = match Ctl::new(&format!("hw:{index}"), true) {
            Ok(control) => control,
            Err(error) => {
                eprintln!("AUDIO_CARD_IGNORED card={index} reason={error}");
                continue;
            }
        };
        let card_name = card
            .get_name()
            .with_context(|| format!("reading ALSA card {index} name"))?;
        let card_id = read_trimmed(format!("/proc/asound/card{index}/id"))
            .unwrap_or_else(|| format!("card-{index}"));
        let usb = usb_identity(index);
        let transport = classify_transport(&card_name, usb.as_ref());
        for pcm_device in DeviceIter::new(&control) {
            let playback_info = control
                .pcm_info(pcm_device as u32, 0, Direction::Playback)
                .ok();
            let capture_info = control
                .pcm_info(pcm_device as u32, 0, Direction::Capture)
                .ok();
            if playback_info.is_none() && capture_info.is_none() {
                continue;
            }
            let address = format!("hw:{index},{pcm_device}");
            let playback = playback_info.as_ref().and_then(|_| {
                probe_stream(&address, Direction::Playback)
                    .map_err(|error| {
                        eprintln!(
                            "AUDIO_STREAM_IGNORED backend={address} direction=playback reason={}",
                            explain_probe_failure(&error)
                        );
                    })
                    .ok()
            });
            let capture = capture_info.as_ref().and_then(|_| {
                probe_stream(&address, Direction::Capture)
                    .map_err(|error| {
                        eprintln!(
                            "AUDIO_STREAM_IGNORED backend={address} direction=capture reason={}",
                            explain_probe_failure(&error)
                        );
                    })
                    .ok()
            });
            if playback.is_none() && capture.is_none() {
                continue;
            }
            let stream_name = playback_info
                .as_ref()
                .or(capture_info.as_ref())
                .and_then(|info| info.get_name().ok())
                .filter(|name| !name.trim().is_empty());
            let id = stable_device_id(index, pcm_device, &card_id, usb.as_ref())?;
            if !ids.insert(id.clone()) {
                bail!("duplicate stable audio device id {id}");
            }
            let descriptor = AudioDeviceDescriptor {
                schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
                id,
                name: stream_name
                    .map(|stream| format!("{card_name} — {stream}"))
                    .unwrap_or_else(|| card_name.clone()),
                backend: AudioBackend::Alsa,
                backend_address: address,
                transport,
                usb: usb.clone(),
                playback,
                capture,
            };
            descriptor
                .validate()
                .with_context(|| format!("validating discovered audio device {}", descriptor.id))?;
            devices.push(descriptor);
        }
    }
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(devices)
}

pub fn open_audio_output(profile: &AudioOutputProfile) -> Result<OpenedAudioOutput> {
    profile
        .validate()
        .context("validating audio output profile")?;
    let devices = discover_audio_devices()?;
    open_audio_output_from_inventory(profile, &devices)
}

pub fn open_audio_output_from_inventory(
    profile: &AudioOutputProfile,
    devices: &[AudioDeviceDescriptor],
) -> Result<OpenedAudioOutput> {
    profile
        .validate()
        .context("validating audio output profile")?;
    let device = resolve_output_device(profile, devices)?.clone();
    profile
        .validate_against(&device)
        .with_context(|| format!("validating output profile against {}", device.id))?;

    // No state is changed until discovery, selection and capability validation
    // have all succeeded. ALSA parameters are then committed as one hw_params
    // transaction and verified before the stream is exposed to the audio loop.
    let pcm = PCM::new(&device.backend_address, Direction::Playback, false)
        .with_context(|| format!("opening ALSA playback {}", device.backend_address))?;
    {
        let parameters = HwParams::any(&pcm)?;
        parameters.set_access(Access::RWInterleaved)?;
        parameters.set_format(to_alsa_format(profile.sample_format))?;
        parameters.set_channels(profile.channels)?;
        parameters.set_rate(profile.sample_rate_hz, ValueOr::Nearest)?;
        parameters.set_period_size(i64::from(profile.period_frames), ValueOr::Nearest)?;
        parameters.set_buffer_size(i64::from(profile.buffer_frames))?;
        pcm.hw_params(&parameters)?;
    }
    let actual = pcm.hw_params_current()?;
    let actual_format = actual.get_format()?;
    let actual_rate = actual.get_rate()?;
    let actual_channels = actual.get_channels()?;
    let actual_period = actual.get_period_size()?;
    let actual_buffer = actual.get_buffer_size()?;
    let expected_format = to_alsa_format(profile.sample_format);
    if actual_format != expected_format
        || actual_rate != profile.sample_rate_hz
        || actual_channels != profile.channels
        || actual_period != i64::from(profile.period_frames)
        || actual_buffer != i64::from(profile.buffer_frames)
    {
        bail!(
            "ALSA negotiated a different output profile: format={actual_format:?} rate={actual_rate} \
             channels={actual_channels} period={actual_period} buffer={actual_buffer}"
        );
    }
    drop(actual);
    configure_playback_software_params(&pcm, profile)?;
    pcm.prepare()?;
    Ok(OpenedAudioOutput {
        pcm,
        device,
        profile: profile.clone(),
    })
}

/// Makes the negotiated hardware buffer an actual scheduling reserve.
///
/// ALSA's device defaults are not uniform: some USB devices auto-start after
/// the first write, even when a larger hardware buffer was requested. That
/// leaves only one period between rendering and the device clock and turns a
/// harmless scheduler hiccup into an underrun storm. Starting only after the
/// complete negotiated buffer has been queued preserves the configured
/// latency while making all of that buffer available as deadline headroom.
fn configure_playback_software_params(pcm: &PCM, profile: &AudioOutputProfile) -> Result<()> {
    let parameters = pcm.sw_params_current()?;
    parameters.set_avail_min(i64::from(profile.period_frames))?;
    parameters.set_start_threshold(i64::from(profile.buffer_frames))?;
    pcm.sw_params(&parameters)?;

    let applied = pcm.sw_params_current()?;
    let actual_avail_min = applied.get_avail_min()?;
    let actual_start_threshold = applied.get_start_threshold()?;
    if actual_avail_min != i64::from(profile.period_frames)
        || actual_start_threshold != i64::from(profile.buffer_frames)
    {
        bail!(
            "ALSA negotiated different playback scheduling: avail_min={actual_avail_min} \
             start_threshold={actual_start_threshold}"
        );
    }
    Ok(())
}

pub fn open_audio_input(profile: &AudioInputProfile) -> Result<OpenedAudioInput> {
    profile
        .validate()
        .context("validating audio input profile")?;
    let devices = discover_audio_devices()?;
    open_audio_input_from_inventory(profile, &devices)
}

pub fn open_audio_input_from_inventory(
    profile: &AudioInputProfile,
    devices: &[AudioDeviceDescriptor],
) -> Result<OpenedAudioInput> {
    profile
        .validate()
        .context("validating audio input profile")?;
    let device = resolve_input_device(profile, devices)?.clone();
    profile
        .validate_against(&device)
        .with_context(|| format!("validating input profile against {}", device.id))?;

    let pcm = PCM::new(&device.backend_address, Direction::Capture, false)
        .with_context(|| format!("opening ALSA capture {}", device.backend_address))?;
    {
        let parameters = HwParams::any(&pcm)?;
        parameters.set_access(Access::RWInterleaved)?;
        parameters.set_format(to_alsa_format(profile.sample_format))?;
        parameters.set_channels(profile.stream_channels())?;
        parameters.set_rate(profile.sample_rate_hz, ValueOr::Nearest)?;
        parameters.set_period_size(i64::from(profile.period_frames), ValueOr::Nearest)?;
        parameters.set_buffer_size(i64::from(profile.buffer_frames))?;
        pcm.hw_params(&parameters)?;
    }
    let actual = pcm.hw_params_current()?;
    let actual_format = actual.get_format()?;
    let actual_rate = actual.get_rate()?;
    let actual_channels = actual.get_channels()?;
    let actual_period = actual.get_period_size()?;
    let actual_buffer = actual.get_buffer_size()?;
    let expected_format = to_alsa_format(profile.sample_format);
    if actual_format != expected_format
        || actual_rate != profile.sample_rate_hz
        || actual_channels != profile.stream_channels()
        || actual_period != i64::from(profile.period_frames)
        || actual_buffer != i64::from(profile.buffer_frames)
    {
        bail!(
            "ALSA negotiated a different input profile: format={actual_format:?} rate={actual_rate} \
             channels={actual_channels} period={actual_period} buffer={actual_buffer}"
        );
    }
    drop(actual);
    pcm.prepare()?;
    Ok(OpenedAudioInput {
        pcm,
        device,
        profile: profile.clone(),
    })
}

fn resolve_output_device<'a>(
    profile: &AudioOutputProfile,
    devices: &'a [AudioDeviceDescriptor],
) -> Result<&'a AudioDeviceDescriptor> {
    let matching = devices
        .iter()
        .filter(|device| device.playback.is_some() && profile.device.matches(device))
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [device] => return Ok(device),
        [] => {}
        _ => {
            bail!(
                "audio selector is ambiguous; matching devices: {}",
                matching
                    .iter()
                    .map(|device| device.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    if profile.fallback == AudioFallbackPolicy::UniqueCompatible {
        let compatible = devices
            .iter()
            .filter(|device| profile.validate_against(device).is_ok())
            .collect::<Vec<_>>();
        match compatible.as_slice() {
            [device] => {
                eprintln!(
                    "AUDIO_FALLBACK selected={} reason=configured-device-missing",
                    device.id
                );
                return Ok(device);
            }
            [] => {}
            _ => bail!(
                "audio fallback is ambiguous; compatible devices: {}",
                compatible
                    .iter()
                    .map(|device| device.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    let available = devices
        .iter()
        .filter(|device| device.playback.is_some())
        .map(|device| device.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("configured audio output was not found; available playback devices: {available}")
}

fn resolve_input_device<'a>(
    profile: &AudioInputProfile,
    devices: &'a [AudioDeviceDescriptor],
) -> Result<&'a AudioDeviceDescriptor> {
    let matching = devices
        .iter()
        .filter(|device| device.capture.is_some() && profile.device.matches(device))
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [device] => return Ok(device),
        [] => {}
        _ => {
            bail!(
                "audio input selector is ambiguous; matching devices: {}",
                matching
                    .iter()
                    .map(|device| device.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    if profile.fallback == AudioFallbackPolicy::UniqueCompatible {
        let compatible = devices
            .iter()
            .filter(|device| profile.validate_against(device).is_ok())
            .collect::<Vec<_>>();
        match compatible.as_slice() {
            [device] => {
                eprintln!(
                    "AUDIO_INPUT_FALLBACK selected={} reason=configured-device-missing",
                    device.id
                );
                return Ok(device);
            }
            [] => {}
            _ => bail!(
                "audio input fallback is ambiguous; compatible devices: {}",
                compatible
                    .iter()
                    .map(|device| device.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    let available = devices
        .iter()
        .filter(|device| device.capture.is_some())
        .map(|device| device.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("configured audio input was not found; available capture devices: {available}")
}

/// Names what ALSA could not.
///
/// A device that exists but refuses to open answers with kernel-internal
/// errno 524, which has no entry in the userspace table — so ALSA prints
/// "Unknown errno (524)" and the log reads like a fault in the host. It is
/// the ordinary state of an output with nothing on the other end of it; on a
/// Raspberry Pi with no display, both HDMI outputs report it on every boot.
/// The device is dropped from the catalogue either way, so the only thing at
/// stake is whether the next person to read this loses an afternoon to it.
fn explain_probe_failure(error: &anyhow::Error) -> String {
    const ENOTSUPP: &str = "Unknown errno (524)";
    let text = format!("{error:#}");
    if text.contains(ENOTSUPP) {
        return text.replace(
            ENOTSUPP,
            "ENOTSUPP — the device exists but cannot be opened, \
             which is what an HDMI output with no display attached reports",
        );
    }
    text
}

fn probe_stream(address: &str, direction: Direction) -> Result<AudioStreamCapabilities> {
    let pcm = PCM::new(address, direction, true)?;
    let parameters = HwParams::any(&pcm)?;
    let sample_formats = [
        (AudioSampleFormat::S16Le, Format::s16()),
        (AudioSampleFormat::S24Le, Format::s24()),
        (AudioSampleFormat::S24_3Le, Format::s24_3()),
        (AudioSampleFormat::S32Le, Format::s32()),
        (AudioSampleFormat::F32Le, Format::float()),
    ]
    .into_iter()
    .filter_map(|(public, alsa)| parameters.test_format(alsa).ok().map(|_| public))
    .collect::<Vec<_>>();
    let sample_rates_hz = COMMON_SAMPLE_RATES
        .into_iter()
        .filter(|rate| parameters.test_rate(*rate).is_ok())
        .collect::<Vec<_>>();
    let capabilities = AudioStreamCapabilities {
        sample_formats,
        sample_rates_hz,
        channels: range(
            parameters.get_channels_min()?,
            parameters.get_channels_max()?,
        )?,
        period_frames: range_frames(
            parameters.get_period_size_min()?,
            parameters.get_period_size_max()?,
        )?,
        buffer_frames: range_frames(
            parameters.get_buffer_size_min()?,
            parameters.get_buffer_size_max()?,
        )?,
    };
    capabilities.validate()?;
    Ok(capabilities)
}

fn range(minimum: u32, maximum: u32) -> Result<AudioValueRange> {
    AudioValueRange::new(minimum, maximum).map_err(Into::into)
}

fn range_frames(minimum: i64, maximum: i64) -> Result<AudioValueRange> {
    let minimum = u32::try_from(minimum).context("negative or oversized ALSA minimum")?;
    let maximum = u32::try_from(maximum).unwrap_or(u32::MAX);
    range(minimum, maximum)
}

fn to_alsa_format(format: AudioSampleFormat) -> Format {
    match format {
        AudioSampleFormat::S16Le => Format::s16(),
        AudioSampleFormat::S24Le => Format::s24(),
        AudioSampleFormat::S24_3Le => Format::s24_3(),
        AudioSampleFormat::S32Le => Format::s32(),
        AudioSampleFormat::F32Le => Format::float(),
    }
}

fn stable_device_id(
    card_index: i32,
    pcm_device: i32,
    card_id: &str,
    usb: Option<&UsbAudioIdentity>,
) -> Result<AudioDeviceId> {
    let value = match usb {
        Some(usb) => {
            let identity = usb.serial.as_deref().map(slug).unwrap_or_else(|| {
                usb_path_key(card_index).unwrap_or_else(|| format!("card-{card_index}"))
            });
            format!(
                "alsa.usb-{:04x}-{:04x}-{identity}.pcm-{pcm_device}",
                usb.vendor_id, usb.product_id
            )
        }
        None => format!("alsa.card-{}.pcm-{pcm_device}", slug(card_id)),
    };
    AudioDeviceId::new(value).map_err(Into::into)
}

fn usb_identity(card_index: i32) -> Option<UsbAudioIdentity> {
    let mut path = fs::canonicalize(format!("/sys/class/sound/card{card_index}/device")).ok()?;
    loop {
        let vendor = read_hex_u16(path.join("idVendor"));
        let product = read_hex_u16(path.join("idProduct"));
        if let (Some(vendor_id), Some(product_id)) = (vendor, product) {
            let serial = read_trimmed(path.join("serial"));
            return Some(UsbAudioIdentity {
                vendor_id,
                product_id,
                serial,
            });
        }
        if !path.pop() {
            return None;
        }
    }
}

fn usb_path_key(card_index: i32) -> Option<String> {
    let mut path = fs::canonicalize(format!("/sys/class/sound/card{card_index}/device")).ok()?;
    loop {
        if path.join("idVendor").is_file() && path.join("idProduct").is_file() {
            return path.file_name().map(|name| slug(&name.to_string_lossy()));
        }
        if !path.pop() {
            return None;
        }
    }
}

fn classify_transport(name: &str, usb: Option<&UsbAudioIdentity>) -> AudioTransport {
    if usb.is_some() {
        AudioTransport::Usb
    } else if name.to_ascii_lowercase().contains("hdmi") {
        AudioTransport::Hdmi
    } else if !name.trim().is_empty() {
        AudioTransport::BuiltIn
    } else {
        AudioTransport::Unknown
    }
}

fn read_hex_u16(path: impl AsRef<Path>) -> Option<u16> {
    u16::from_str_radix(read_trimmed(path)?.trim_start_matches("0x"), 16).ok()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !output.is_empty() {
                output.push('-');
            }
            output.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
        if output.len() >= 64 {
            break;
        }
    }
    if output.is_empty() {
        "unknown".into()
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_audio_api::{AudioDeviceSelector, AudioFallbackPolicy};

    fn profile(selector: AudioDeviceSelector) -> AudioOutputProfile {
        AudioOutputProfile {
            device: selector,
            fallback: AudioFallbackPolicy::None,
            sample_format: AudioSampleFormat::S32Le,
            sample_rate_hz: 48_000,
            channels: 2,
            period_frames: 128,
            buffer_frames: 384,
        }
    }

    fn descriptor(id: &str, usb: Option<UsbAudioIdentity>) -> AudioDeviceDescriptor {
        AudioDeviceDescriptor {
            schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
            id: AudioDeviceId::new(id).unwrap(),
            name: "Device".into(),
            backend: AudioBackend::Alsa,
            backend_address: "hw:3,0".into(),
            transport: if usb.is_some() {
                AudioTransport::Usb
            } else {
                AudioTransport::BuiltIn
            },
            usb,
            playback: Some(AudioStreamCapabilities {
                sample_formats: vec![AudioSampleFormat::S32Le],
                sample_rates_hz: vec![48_000],
                channels: AudioValueRange::new(2, 2).unwrap(),
                period_frames: AudioValueRange::new(64, 1024).unwrap(),
                buffer_frames: AudioValueRange::new(128, 4096).unwrap(),
            }),
            capture: None,
        }
    }

    #[test]
    fn stable_id_uses_usb_serial_instead_of_card_index() {
        let usb = UsbAudioIdentity {
            vendor_id: 0x1235,
            product_id: 0x8211,
            serial: Some("Y7F5 QXY".into()),
        };
        let first = stable_device_id(3, 0, "USB", Some(&usb)).unwrap();
        let second = stable_device_id(8, 0, "USB2", Some(&usb)).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.as_str(), "alsa.usb-1235-8211-y7f5-qxy.pcm-0");
    }

    #[test]
    fn exact_selector_never_silently_uses_another_device() {
        let devices = vec![descriptor("alsa.card-headphones.pcm-0", None)];
        let profile = profile(AudioDeviceSelector::Id {
            id: AudioDeviceId::new("alsa.usb-missing.pcm-0").unwrap(),
        });
        assert!(resolve_output_device(&profile, &devices).is_err());
    }

    #[test]
    fn fallback_requires_one_compatible_device() {
        let mut profile = profile(AudioDeviceSelector::Id {
            id: AudioDeviceId::new("alsa.usb-missing.pcm-0").unwrap(),
        });
        profile.fallback = AudioFallbackPolicy::UniqueCompatible;
        let one = vec![descriptor("alsa.card-headphones.pcm-0", None)];
        assert_eq!(resolve_output_device(&profile, &one).unwrap().id, one[0].id);
        let two = vec![
            descriptor("alsa.card-headphones.pcm-0", None),
            descriptor("alsa.card-hdmi.pcm-0", None),
        ];
        assert!(resolve_output_device(&profile, &two).is_err());
    }

    #[test]
    fn slug_is_bounded_and_safe_for_persistence() {
        assert_eq!(slug("Scarlett Solo USB / Main"), "scarlett-solo-usb-main");
        assert_eq!(slug("---"), "unknown");
    }

    /// The line a headless Raspberry Pi prints twice on every boot.
    #[test]
    fn a_device_that_refuses_to_open_is_named_rather_than_numbered() {
        let alsa =
            anyhow::anyhow!("ALSA function 'snd_pcm_open' failed with error 'Unknown errno (524)'");
        let explained = explain_probe_failure(&alsa);
        assert!(
            explained.contains("ENOTSUPP"),
            "the errno was left as a number: {explained}"
        );
        assert!(
            explained.contains("HDMI output with no display"),
            "the usual cause was left unsaid: {explained}"
        );
        assert!(
            !explained.contains("Unknown errno"),
            "the unhelpful text survived: {explained}"
        );
    }

    /// Anything else is passed through untouched: guessing at an error we do
    /// not recognise would be worse than the number.
    #[test]
    fn an_unrecognised_failure_is_reported_as_it_came() {
        let other = anyhow::anyhow!("ALSA function 'snd_pcm_open' failed with error 'EBUSY'");
        assert_eq!(
            explain_probe_failure(&other),
            "ALSA function 'snd_pcm_open' failed with error 'EBUSY'"
        );
    }
}
