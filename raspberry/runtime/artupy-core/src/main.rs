use anyhow::{Context, Result, bail};
use artupy_core::{LoadedPlugin, PluginPackage};
use artupy_plugin_api::{ParameterKind, PluginKind};
use std::env;
use std::path::{Path, PathBuf};

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
        "smoke" => {
            let (package, binary) = parse_smoke_arguments(&arguments[1..])?;
            smoke(&package, binary.as_deref())
        }
        _ => bail!(
            "usage:\n  artupy-core inspect PACKAGE\n  \
             artupy-core smoke PACKAGE [--library FILE]"
        ),
    }
}

fn parse_smoke_arguments(arguments: &[String]) -> Result<(PathBuf, Option<PathBuf>)> {
    let package = arguments
        .first()
        .context("smoke requires a plugin package")?;
    let mut binary = None;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--library" => {
                index += 1;
                binary = Some(PathBuf::from(
                    arguments.get(index).context("--library requires a file")?,
                ));
            }
            option => bail!("unknown smoke option {option}"),
        }
        index += 1;
    }
    Ok((PathBuf::from(package), binary))
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
    Ok(())
}

fn smoke(package_path: &Path, binary: Option<&Path>) -> Result<()> {
    let package = PluginPackage::open(package_path)?;
    // SAFETY: smoke is an explicit native plugin execution command.
    let plugin = unsafe { LoadedPlugin::load(&package, binary) }?;
    println!(
        "PLUGIN_LOADED id={} parameters={} pages={} presets={}",
        plugin.descriptor().id,
        plugin.parameters().parameters.len(),
        plugin.parameters().pages.len(),
        plugin.presets().presets.len()
    );

    let mut instance = plugin.create_instance()?;
    if let Some(preset) = plugin.presets().presets.first() {
        instance.load_preset(&preset.id)?;
        println!("PRESET_LOADED id={} name={:?}", preset.id, preset.name);
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

    let frames = 32_u32;
    let input = vec![0.25_f32; frames as usize * input_channels as usize];
    let mut output = vec![0.0_f32; frames as usize * output_channels as usize];
    instance.process_interleaved(
        &input,
        &mut output,
        frames,
        input_channels,
        output_channels,
        &[],
        &[],
    )?;
    let peak = output
        .iter()
        .fold(0.0_f32, |maximum, sample| maximum.max(sample.abs()));
    let state = instance.save_state()?;
    instance.load_state(&state)?;
    instance.deactivate()?;
    println!("PLUGIN_SMOKE_OK peak={peak:.6} state_bytes={}", state.len());
    Ok(())
}
