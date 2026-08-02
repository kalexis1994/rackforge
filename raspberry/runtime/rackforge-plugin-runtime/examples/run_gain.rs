use anyhow::{Context, Result};
use rackforge_plugin_runtime::{PortableEngine, RuntimeLimits};
use std::{env, fs};

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: run_gain PATH_TO_COMPONENT.wasm [CODE_CACHE_DIRECTORY]")?;
    let cache = env::args()
        .nth(2)
        .unwrap_or_else(|| "target/rackforge-portable-cache".into());
    let bytes = fs::read(&path).with_context(|| format!("reading {path}"))?;
    let engine = PortableEngine::with_cache(RuntimeLimits::default(), &cache)?;
    let module = engine.compile(&bytes)?;
    let mut instance = module.instantiate()?;
    instance.prepare(48_000.0, 64, 2)?;
    instance.set_parameter(0, 0.5)?;
    let input = [1.0, -1.0, 0.25, -0.25];
    let mut output = [0.0; 4];
    instance.process_interleaved(&input, &mut output, 2)?;
    anyhow::ensure!(output == [0.5, -0.5, 0.125, -0.125]);
    let state = instance.save_state()?;
    instance.load_preset("mute")?;
    instance.process_interleaved(&input, &mut output, 2)?;
    anyhow::ensure!(output == [0.0; 4]);
    instance.load_state(&state)?;
    instance.process_interleaved(&input, &mut output, 2)?;
    anyhow::ensure!(output == [0.5, -0.5, 0.125, -0.125]);
    println!("RACKFORGE_WASM_OK input={input:?} output={output:?} artifact={path} cache={cache}");
    Ok(())
}
