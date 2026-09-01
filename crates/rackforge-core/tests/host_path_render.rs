//! Renders the Concert Grand through the host's real process path -- the
//! packaged wasm-v1 component behind `PluginInstance::process_interleaved`
//! -- so a change to the host's MIDI plumbing can be proved to leave every
//! sample untouched. The plug-in's own reference render (`CG_RENDER_DIR`)
//! runs its Rust natively and never crosses the host; this one does nothing
//! else.
//!
//! Set `RF_HOST_RENDER_DIR` to write one raw little-endian interleaved-f32
//! file per note/velocity pair of the reference set (thirty notes from A0 in
//! minor thirds, at velocities 50 and 125); unset, the test is a no-op. Run
//! with the wasm built:
//!
//!     cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
//!     RF_HOST_RENDER_DIR=... cargo test -p rackforge-core --release --test host_path_render -- --include-ignored
#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_plugin_api::abi::MidiEventV1;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const RATE: f64 = 48_000.0;
const FRAMES: u32 = 512;
/// About one second held, then two of tail: enough to cover the strike, the
/// two-stage decay and the damper landing.
const HOLD_BLOCKS: usize = 94;
const TAIL_BLOCKS: usize = 188;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[ignore]
fn render_through_the_host() {
    let Ok(out) = std::env::var("RF_HOST_RENDER_DIR") else {
        return;
    };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let wasm = repo.join("target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm");
    assert!(wasm.is_file(), "build the wasm first: {}", wasm.display());
    let root = std::env::temp_dir().join(format!("rackforge-host-render-{}", std::process::id()));
    copy_tree(&repo.join("plugins/concert-grand/package"), &root);
    fs::copy(&wasm, root.join("component.wasm")).unwrap();
    let package = PluginPackage::open(&root).unwrap();
    // SAFETY: portable wasm-v1 packages execute inside the sandbox.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
    let out = Path::new(&out);
    fs::create_dir_all(out).unwrap();

    for note in (21u8..=108).step_by(3) {
        for velocity in [50u8, 125] {
            let mut instance = loaded.create_instance().unwrap();
            instance.activate(RATE, FRAMES, 0, 2).unwrap();
            let mut output = vec![0.0f32; FRAMES as usize * 2];
            let mut bytes = Vec::with_capacity((HOLD_BLOCKS + TAIL_BLOCKS) * output.len() * 4);
            let on = MidiEventV1 {
                frame: 0,
                length: 3,
                data: [0x90, note, velocity],
            };
            let off = MidiEventV1 {
                frame: 0,
                length: 3,
                data: [0x80, note, 64],
            };
            for block in 0..HOLD_BLOCKS + TAIL_BLOCKS {
                let events: &[MidiEventV1] = if block == 0 {
                    std::slice::from_ref(&on)
                } else if block == HOLD_BLOCKS {
                    std::slice::from_ref(&off)
                } else {
                    &[]
                };
                instance
                    .process_interleaved(&[], &mut output, FRAMES, 0, 2, events, &[])
                    .unwrap();
                for sample in &output {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
            fs::write(out.join(format!("host{note:03}v{velocity}.f32")), bytes).unwrap();
        }
    }
    fs::remove_dir_all(root).ok();
}
