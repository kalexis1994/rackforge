use anyhow::{Context, Result, bail};
#[cfg(target_os = "linux")]
use rackforge_audio_api::{
    AudioDeviceSelector, AudioFallbackPolicy, AudioInputDocument, AudioInputProfile,
    AudioOutputDocument, AudioOutputProfile, AudioSampleFormat,
};
use rackforge_core::{LoadedPlugin, PluginPackage, PluginStorage};
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_api::{ParameterKind, PluginKind, ProgramDocument, ProgramEditRequest};
#[cfg(target_os = "linux")]
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(target_os = "linux")]
mod audio_instance;

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("");
    match command {
        "inspect" if arguments.len() == 2 => inspect(Path::new(&arguments[1])),
        "validate-resource" if arguments.len() == 4 => validate_resource(
            Path::new(&arguments[1]),
            &arguments[2],
            Path::new(&arguments[3]),
        ),
        "smoke" => {
            let (package, binary, resources, preset, data_root) =
                parse_plugin_arguments("smoke", &arguments[1..])?;
            smoke(
                &package,
                binary.as_deref(),
                &resources,
                preset.as_deref(),
                data_root.as_deref(),
            )
        }
        "stress" => {
            let (plugin, voices, blocks, frames) = parse_stress_arguments(&arguments[1..])?;
            let (package, binary, resources, preset, data_root) = plugin;
            stress(
                &package,
                binary.as_deref(),
                &resources,
                preset.as_deref(),
                data_root.as_deref(),
                voices,
                blocks,
                frames,
            )
        }
        "live" => run_live(&arguments[1..]),
        "resume" if arguments.len() == 2 => resume(Path::new(&arguments[1])),
        #[cfg(target_os = "linux")]
        "audio-list" if arguments.len() == 1 => list_audio_devices(),
        "plugin-init" if arguments.len() == 3 => {
            init_plugin(Path::new(&arguments[1]), &arguments[2])
        }
        "program-save" if arguments.len() == 4 => save_program(
            Path::new(&arguments[1]),
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
        ),
        _ => bail!(
            "usage:\n  rackforge-core inspect PACKAGE\n  \
             rackforge-core validate-resource PACKAGE RESOURCE_ID FILE\n  \
             rackforge-core smoke PACKAGE [--library FILE] [--resource ID=PATH]... \
             [--preset ID] [--data-root DIRECTORY]\n  \
             rackforge-core stress PACKAGE [--library FILE] [--resource ID=PATH]... \
             [--preset ID] [--data-root DIRECTORY] [--voices COUNT] \
             [--blocks COUNT] [--frames COUNT]\n  \
             rackforge-core live PACKAGE [--library FILE] [--resource ID=PATH]... \
             [--preset ID] [--data-root DIRECTORY]\n  \
             rackforge-core resume STARTUP_CONFIG\n  \
             rackforge-core audio-list\n  \
             rackforge-core plugin-init DATA_ROOT PLUGIN_ID\n  \
             rackforge-core program-save DATA_ROOT RELATIVE_PATH DOCUMENT"
        ),
    }
}

fn validate_resource(package_path: &Path, resource_id: &str, path: &Path) -> Result<()> {
    let package = PluginPackage::open(package_path)?;
    let plugin = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }?;
    plugin.validate_resource_file(resource_id, path)?;
    println!(
        "RESOURCE_VALID plugin={} resource={} path={}",
        plugin.descriptor().id,
        resource_id,
        path.display()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
const AUDIO_STARTUP_SCHEMA_VERSION: u32 = 3;

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioStartupConfig {
    schema_version: u32,
    package: PathBuf,
    #[serde(default)]
    resources: BTreeMap<String, PathBuf>,
    #[serde(default)]
    preset: Option<String>,
    data_root: PathBuf,
    #[serde(default)]
    audio: Option<AudioStartupSection>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioStartupSection {
    output: AudioOutputProfile,
    #[serde(default)]
    input: Option<AudioInputProfile>,
}

#[cfg(target_os = "linux")]
fn resume(path: &Path) -> Result<()> {
    let _audio_instance = audio_instance::AudioEngineGuard::acquire(&audio_engine_lock_path())?;
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading audio startup config {}", path.display()))?;
    let config: AudioStartupConfig = toml::from_str(&text)
        .with_context(|| format!("parsing audio startup config {}", path.display()))?;
    if !matches!(config.schema_version, 1 | 2 | AUDIO_STARTUP_SCHEMA_VERSION) {
        bail!(
            "unsupported audio startup schema {} in {}",
            config.schema_version,
            path.display()
        );
    }
    if !config.package.is_absolute() || !config.data_root.is_absolute() {
        bail!("audio startup package and data_root must be absolute paths");
    }
    if config.resources.values().any(|path| !path.is_absolute()) {
        bail!("audio startup resource paths must be absolute");
    }
    let (configured_audio_output, configured_audio_input) =
        match (config.schema_version, config.audio) {
            (1, None) => {
                eprintln!("AUDIO_CONFIG_MIGRATED from_schema=1 to_schema=3");
                (legacy_scarlett_profile(), None)
            }
            (AUDIO_STARTUP_SCHEMA_VERSION, Some(audio)) | (2, Some(audio)) | (1, Some(audio)) => {
                (audio.output, audio.input)
            }
            (_, None) => bail!("audio startup schema 2 or 3 requires [audio.output]"),
            _ => unreachable!(),
        };
    let audio_state_path = audio_output_state_path();
    let audio_output =
        load_persisted_audio_output(&audio_state_path).unwrap_or(configured_audio_output);
    audio_output
        .validate()
        .context("validating [audio.output]")?;
    let audio_input =
        load_persisted_audio_input(&audio_input_state_path()).or(configured_audio_input);
    if let Some(input) = &audio_input {
        input.validate().context("validating [audio.input]")?;
    }
    rackforge_core::live::run(rackforge_core::live::LiveConfig {
        package: config.package,
        binary: None,
        resources: config.resources,
        preset: config.preset,
        data_root: Some(config.data_root),
        audio_output,
        audio_input,
        audio_state_path,
    })
}

#[cfg(target_os = "linux")]
fn audio_output_state_path() -> PathBuf {
    env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("rackforge")
        })
        .join("state")
        .join("audio-output.toml")
}

#[cfg(target_os = "linux")]
fn audio_input_state_path() -> PathBuf {
    audio_output_state_path().with_file_name("audio-input.toml")
}

#[cfg(target_os = "linux")]
fn audio_engine_lock_path() -> PathBuf {
    audio_output_state_path().with_file_name("audio-engine.lock")
}

#[cfg(target_os = "linux")]
fn load_persisted_audio_output(path: &Path) -> Option<AudioOutputProfile> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            eprintln!("AUDIO_STATE_IGNORED path={} reason={error}", path.display());
            return None;
        }
    };
    match toml::from_str::<AudioOutputDocument>(&text)
        .context("parsing persisted audio output")
        .and_then(|document| {
            document
                .validate()
                .context("validating persisted audio output")?;
            Ok(document.output)
        }) {
        Ok(profile) => {
            println!("AUDIO_STATE_RESTORED path={}", path.display());
            Some(profile)
        }
        Err(error) => {
            eprintln!(
                "AUDIO_STATE_IGNORED path={} reason={error:#}",
                path.display()
            );
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn load_persisted_audio_input(path: &Path) -> Option<AudioInputProfile> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            eprintln!(
                "AUDIO_INPUT_STATE_IGNORED path={} reason={error}",
                path.display()
            );
            return None;
        }
    };
    match toml::from_str::<AudioInputDocument>(&text)
        .context("parsing persisted audio input")
        .and_then(|document| {
            document
                .validate()
                .context("validating persisted audio input")?;
            Ok(document.input)
        }) {
        Ok(profile) => {
            println!("AUDIO_INPUT_STATE_RESTORED path={}", path.display());
            Some(profile)
        }
        Err(error) => {
            eprintln!(
                "AUDIO_INPUT_STATE_IGNORED path={} reason={error:#}",
                path.display()
            );
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn legacy_scarlett_profile() -> AudioOutputProfile {
    AudioOutputProfile {
        device: AudioDeviceSelector::Usb {
            vendor_id: 0x1235,
            product_id: 0x8211,
            serial: None,
        },
        fallback: AudioFallbackPolicy::None,
        sample_format: AudioSampleFormat::S32Le,
        sample_rate_hz: 48_000,
        channels: 2,
        period_frames: 128,
        buffer_frames: 384,
    }
}

#[cfg(target_os = "linux")]
fn list_audio_devices() -> Result<()> {
    let devices = rackforge_core::audio::discover_audio_devices()?;
    println!("{}", serde_json::to_string_pretty(&devices)?);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn resume(_path: &Path) -> Result<()> {
    bail!("rackforge-core resume is available on Linux only")
}

fn init_plugin(data_root: &Path, plugin_id: &str) -> Result<()> {
    let directory = PluginStorage::new(data_root).ensure_plugin(plugin_id)?;
    println!(
        "PLUGIN_DATA_READY id={} root={}",
        plugin_id,
        directory.root.display()
    );
    Ok(())
}

fn save_program(data_root: &Path, relative: &Path, document: &Path) -> Result<()> {
    let bytes =
        std::fs::read(document).with_context(|| format!("reading {}", document.display()))?;
    let program: ProgramDocument = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing {}", document.display()))?;
    let destination = PluginStorage::new(data_root).save_program(relative, &program)?;
    println!(
        "PROGRAM_SAVED id={} plugin={} path={}",
        program.id,
        program.plugin_id,
        destination.display()
    );
    Ok(())
}

type PluginArguments = (
    PathBuf,
    Option<PathBuf>,
    BTreeMap<String, PathBuf>,
    Option<String>,
    Option<PathBuf>,
);

fn parse_plugin_arguments(command: &str, arguments: &[String]) -> Result<PluginArguments> {
    let package = arguments
        .first()
        .with_context(|| format!("{command} requires a plugin package"))?;
    let mut binary = None;
    let mut resources = BTreeMap::new();
    let mut preset = None;
    let mut data_root = None;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--library" => {
                index += 1;
                binary = Some(PathBuf::from(
                    arguments.get(index).context("--library requires a file")?,
                ));
            }
            "--resource" => {
                index += 1;
                let assignment = arguments
                    .get(index)
                    .context("--resource requires ID=PATH")?;
                let (id, path) = assignment
                    .split_once('=')
                    .context("--resource requires ID=PATH")?;
                if id.is_empty() || path.is_empty() {
                    bail!("--resource requires non-empty ID=PATH");
                }
                if resources
                    .insert(id.to_owned(), PathBuf::from(path))
                    .is_some()
                {
                    bail!("resource {id:?} was supplied more than once");
                }
            }
            "--preset" => {
                index += 1;
                preset = Some(
                    arguments
                        .get(index)
                        .context("--preset requires an ID")?
                        .clone(),
                );
            }
            "--data-root" => {
                index += 1;
                data_root = Some(PathBuf::from(
                    arguments
                        .get(index)
                        .context("--data-root requires a directory")?,
                ));
            }
            option => bail!("unknown {command} option {option}"),
        }
        index += 1;
    }
    Ok((PathBuf::from(package), binary, resources, preset, data_root))
}

fn parse_stress_arguments(arguments: &[String]) -> Result<(PluginArguments, u8, u32, u32)> {
    let mut common = Vec::new();
    let mut voices = 28_u8;
    let mut blocks = 32_u32;
    let mut frames = 256_u32;
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        if matches!(option, "--voices" | "--blocks" | "--frames") {
            index += 1;
            let value = arguments
                .get(index)
                .with_context(|| format!("{option} requires an integer"))?;
            match option {
                "--voices" => {
                    voices = value.parse().context("--voices must be an integer")?;
                    if voices == 0 || voices > 128 {
                        bail!("--voices must be in 1..=128");
                    }
                }
                "--blocks" => {
                    blocks = value.parse().context("--blocks must be an integer")?;
                    if blocks == 0 || blocks > 10_000 {
                        bail!("--blocks must be in 1..=10000");
                    }
                }
                "--frames" => {
                    frames = value.parse().context("--frames must be an integer")?;
                    if frames == 0 || frames > 4_096 {
                        bail!("--frames must be in 1..=4096");
                    }
                }
                _ => unreachable!(),
            }
        } else {
            common.push(arguments[index].clone());
        }
        index += 1;
    }
    Ok((
        parse_plugin_arguments("stress", &common)?,
        voices,
        blocks,
        frames,
    ))
}

fn inspect(path: &Path) -> Result<()> {
    let package = PluginPackage::open(path)?;
    let manifest = package.manifest();
    println!(
        "PLUGIN_PACKAGE_VALID id={} name={:?} version={} kind={:?} api={}.{}",
        manifest.id,
        manifest.name,
        manifest.version,
        manifest.kind,
        manifest.api.major,
        manifest.api.minor
    );
    for (platform, binary) in &manifest.binaries {
        println!("BINARY platform={platform} path={binary}");
    }
    for resource in &manifest.resources {
        println!(
            "RESOURCE id={} kind={:?} required={} name={:?}",
            resource.id, resource.kind, resource.required, resource.name
        );
    }
    Ok(())
}

fn smoke(
    package_path: &Path,
    binary: Option<&Path>,
    resources: &BTreeMap<String, PathBuf>,
    requested_preset: Option<&str>,
    data_root: Option<&Path>,
) -> Result<()> {
    let package = PluginPackage::open(package_path)?;
    // SAFETY: smoke is an explicit native plugin execution command.
    let plugin = unsafe { LoadedPlugin::load(&package, binary, resources, data_root) }?;
    println!(
        "PLUGIN_LOADED id={} parameters={} pages={} presets={}",
        plugin.descriptor().id,
        plugin.parameters().parameters.len(),
        plugin.parameters().pages.len(),
        plugin.presets().presets.len()
    );

    let mut instance = plugin.create_instance()?;
    let presets = instance.preset_catalog()?;
    println!("DYNAMIC_PRESETS count={}", presets.presets.len());
    let preset = match requested_preset {
        Some(id) => Some(
            presets
                .presets
                .iter()
                .chain(plugin.presets().presets.iter())
                .find(|preset| preset.id == id)
                .with_context(|| format!("plugin does not declare preset {id:?}"))?,
        ),
        None => presets.presets.first(),
    };
    if let Some(preset) = preset {
        instance.load_preset(&preset.id)?;
        println!("PRESET_LOADED id={} name={:?}", preset.id, preset.name);
    }
    if instance.supports_program_editing() {
        let editable = preset
            .filter(|preset| preset.editable)
            .or_else(|| presets.presets.iter().find(|preset| preset.editable));
        let request = ProgramEditRequest::new(editable.map(|preset| preset.id.clone()));
        let prepared = instance.begin_program_edit(&request)?;
        let editor = instance.program_editor_view(&prepared.document)?;
        println!(
            "PROGRAM_EDIT_READY source={:?} draft={} pages={}",
            request.program_id,
            prepared.document.id,
            editor.pages.len()
        );
    }
    let input_channels = match plugin.manifest().kind {
        PluginKind::Effect => 2,
        PluginKind::Instrument | PluginKind::MidiProcessor => 0,
    };
    let output_channels = match plugin.manifest().kind {
        PluginKind::MidiProcessor => 0,
        PluginKind::Instrument | PluginKind::Effect => 2,
    };
    instance.activate(48_000.0, 128, input_channels, output_channels)?;

    if let Some(parameter) = plugin.parameters().parameters.iter().find(|parameter| {
        !parameter.flags.read_only
            && matches!(
                parameter.kind,
                ParameterKind::Float { .. } | ParameterKind::Integer { .. }
            )
    }) {
        let value = match parameter.kind {
            ParameterKind::Float {
                minimum, maximum, ..
            } => minimum + (maximum - minimum) * 0.25,
            ParameterKind::Integer {
                minimum, maximum, ..
            } => (minimum + (maximum - minimum) / 4) as f64,
            _ => unreachable!(),
        };
        instance.set_parameter(parameter.index, value)?;
        println!(
            "PARAMETER_ROUNDTRIP id={} value={:.6}",
            parameter.id,
            instance.get_parameter(parameter.index)?
        );
    }

    let frames = 128_u32;
    let input = vec![0.25_f32; frames as usize * input_channels as usize];
    let mut output = vec![0.0_f32; frames as usize * output_channels as usize];
    let note_on = [MidiEventV1 {
        frame: 0,
        length: 3,
        data: [0x90, 60, 100],
    }];
    let mut peak = 0.0_f32;
    for block in 0..16 {
        output.fill(0.0);
        let midi = if block == 0 && plugin.manifest().kind == PluginKind::Instrument {
            note_on.as_slice()
        } else {
            &[]
        };
        instance.process_interleaved(
            &input,
            &mut output,
            frames,
            input_channels,
            output_channels,
            midi,
            &[],
        )?;
        peak = output
            .iter()
            .fold(peak, |maximum, sample| maximum.max(sample.abs()));
    }
    let state = instance.save_state()?;
    instance.load_state(&state)?;
    instance.deactivate()?;
    println!("PLUGIN_SMOKE_OK peak={peak:.6} state_bytes={}", state.len());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stress(
    package_path: &Path,
    binary: Option<&Path>,
    resources: &BTreeMap<String, PathBuf>,
    requested_preset: Option<&str>,
    data_root: Option<&Path>,
    voices: u8,
    blocks: u32,
    frames: u32,
) -> Result<()> {
    let package = PluginPackage::open(package_path)?;
    // SAFETY: stress is an explicit plugin execution command.
    let plugin = unsafe { LoadedPlugin::load(&package, binary, resources, data_root) }?;
    if plugin.manifest().kind != PluginKind::Instrument {
        bail!("stress currently requires an instrument plugin");
    }
    let mut instance = plugin.create_instance()?;
    let presets = instance.preset_catalog()?;
    let preset = match requested_preset {
        Some(id) => presets
            .presets
            .iter()
            .chain(plugin.presets().presets.iter())
            .find(|preset| preset.id == id)
            .with_context(|| format!("plugin does not declare preset {id:?}"))?,
        None => presets
            .presets
            .first()
            .context("instrument does not expose a stress-test preset")?,
    };
    instance.load_preset(&preset.id)?;
    instance.activate(48_000.0, frames, 0, 2)?;

    let note_ons = (0..voices)
        .map(|voice| MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x90, 36 + voice % 60, 100],
        })
        .collect::<Vec<_>>();
    let mut output = vec![0.0_f32; frames as usize * 2];
    let mut peak = 0.0_f32;
    let mut maximum_fuel = 0_u64;
    let started = Instant::now();
    for block in 0..blocks {
        output.fill(0.0);
        let events = if block == 0 { note_ons.as_slice() } else { &[] };
        let result = instance.process_interleaved(&[], &mut output, frames, 0, 2, events, &[]);
        let fuel = instance.last_realtime_fuel_consumed().unwrap_or(0);
        maximum_fuel = maximum_fuel.max(fuel);
        result
            .with_context(|| format!("stress block {block} failed after consuming {fuel} fuel"))?;
        peak = output
            .iter()
            .fold(peak, |maximum, sample| maximum.max(sample.abs()));
    }
    let elapsed = started.elapsed();
    let rendered_seconds = f64::from(blocks) * f64::from(frames) / 48_000.0;
    let realtime_ratio = elapsed.as_secs_f64() / rendered_seconds;
    instance.deactivate()?;
    println!(
        "PLUGIN_STRESS_OK id={} preset={} voices={} blocks={} frames={} max_fuel={} peak={peak:.6} realtime_ratio={realtime_ratio:.3}",
        plugin.descriptor().id,
        preset.id,
        voices,
        blocks,
        frames,
        maximum_fuel,
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_live(arguments: &[String]) -> Result<()> {
    let (package, binary, resources, preset, data_root) =
        parse_plugin_arguments("live", arguments)?;
    let _audio_instance = audio_instance::AudioEngineGuard::acquire(&audio_engine_lock_path())?;
    rackforge_core::live::run(rackforge_core::live::LiveConfig {
        package,
        binary,
        resources,
        preset,
        data_root,
        audio_output: legacy_scarlett_profile(),
        audio_input: None,
        audio_state_path: audio_output_state_path(),
    })
}

#[cfg(not(target_os = "linux"))]
fn run_live(_arguments: &[String]) -> Result<()> {
    bail!("rackforge-core live is available on Linux only")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn schema_two_requires_and_parses_a_typed_audio_profile() {
        let config: AudioStartupConfig = toml::from_str(
            r#"
schema_version = 2
package = "/opt/rackforge/plugin"
data_root = "/var/lib/rackforge"

[audio.output]
fallback = "none"
sample_format = "s32_le"
sample_rate_hz = 48000
channels = 2
period_frames = 128
buffer_frames = 384

[audio.output.device]
mode = "usb"
vendor_id = 0x1235
product_id = 0x8211
"#,
        )
        .unwrap();
        let output = config.audio.unwrap().output;
        output.validate().unwrap();
        assert_eq!(output.sample_rate_hz, 48_000);
        assert_eq!(output.fallback, AudioFallbackPolicy::None);
    }

    #[test]
    fn schema_three_parses_opt_in_capture_with_physical_channels() {
        let config: AudioStartupConfig = toml::from_str(
            r#"
schema_version = 3
package = "/opt/rackforge/plugin"
data_root = "/var/lib/rackforge"

[audio.output]
fallback = "none"
sample_format = "s32_le"
sample_rate_hz = 48000
channels = 2
period_frames = 128
buffer_frames = 384

[audio.output.device]
mode = "usb"
vendor_id = 0x1235
product_id = 0x8211

[audio.input]
fallback = "none"
sample_format = "s32_le"
sample_rate_hz = 48000
channels = [2]
period_frames = 128
buffer_frames = 384
gain_db = 3

[audio.input.device]
mode = "usb"
vendor_id = 0x1235
product_id = 0x8211
"#,
        )
        .unwrap();
        let input = config.audio.unwrap().input.unwrap();
        input.validate().unwrap();
        assert_eq!(input.channels, vec![2]);
        assert_eq!(input.gain_db, 3);
    }

    #[test]
    fn schema_one_remains_parseable_for_explicit_migration() {
        let config: AudioStartupConfig = toml::from_str(
            r#"
schema_version = 1
package = "/opt/rackforge/plugin"
data_root = "/var/lib/rackforge"
"#,
        )
        .unwrap();
        assert!(config.audio.is_none());
        legacy_scarlett_profile().validate().unwrap();
    }
}
