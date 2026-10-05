//! Which of a portable package's two forms the host runs.
//!
//! A package laid down by the release's official set, carrying a native
//! build for this platform, runs that build; every other package runs its
//! component; and a build that will not load is passed over for the
//! component, never a reason the package fails. The store's record is the
//! only thing that decides trust, so these tests write a store.

#![cfg(not(target_arch = "wasm32"))]

use rackforge_core::{LoadedPlugin, PluginPackage};
use rackforge_plugin_api::host_platform_key;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const ID: &str = "org.rackforge.gain-portable";
const VERSION: &str = "0.1.0";

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A component that answers every call and writes silence: enough to load.
const QUIET: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "rackforge_abi_version") (result i32) i32.const 65538)
  (func (export "rackforge_input_ptr") (result i32) i32.const 0)
  (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
  (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
  (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
  (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
  (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
  (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
  (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
  (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
  (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
  (func (export "rackforge_initialize") (result i32) i32.const 0)
  (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
  (func (export "rackforge_set_parameter") (param i32 f64) (result i32) i32.const 0)
  (func (export "rackforge_get_parameter") (param i32) (result f64) f64.const 0)
  (func (export "rackforge_reset") (result i32) i32.const 0)
  (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
  (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
  (func (export "rackforge_resource_end") (result i32) i32.const -3)
  (func (export "rackforge_load_preset") (param i32) (result i32) i32.const 0)
  (func (export "rackforge_save_state") (result i32) i32.const 0)
  (func (export "rackforge_load_state") (param i32) (result i32) i32.const 0)
  (func (export "rackforge_process") (param i32 i32 i32 i32 i32) (result i32) i32.const 0)
)
"#;

/// The portable gain, installed in a fresh store with a record from
/// `source`, its component `component` and its native build for this
/// platform `native`.
fn install(source: &str, component: &[u8], native: &[u8]) -> PathBuf {
    let store = std::env::temp_dir().join(format!(
        "rackforge-native-selection-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let package = store.join("packages").join(ID).join(VERSION);
    let shipped = repository().join("plugins/gain-portable/package");
    fs::create_dir_all(package.join("metadata")).unwrap();
    for file in ["parameters.json", "presets.json", "runtime.json"] {
        fs::copy(
            shipped.join("metadata").join(file),
            package.join("metadata").join(file),
        )
        .unwrap();
    }
    let library = format!(
        "native/{}/{}gain{}",
        host_platform_key(),
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let mut manifest = fs::read_to_string(shipped.join("rackforge-plugin.toml")).unwrap();
    manifest.push_str(&format!(
        "\n[binaries]\n{} = \"{library}\"\n",
        host_platform_key()
    ));
    fs::write(package.join("rackforge-plugin.toml"), manifest).unwrap();
    fs::write(package.join("component.wasm"), component).unwrap();
    fs::create_dir_all(package.join(&library).parent().unwrap()).unwrap();
    fs::write(package.join(&library), native).unwrap();
    fs::create_dir_all(store.join("records").join(ID)).unwrap();
    fs::write(
        store.join("records").join(ID).join(format!("{VERSION}.json")),
        format!(
            r#"{{"schema_version":1,"plugin_id":"{ID}","version":"{VERSION}","platform":"wasm-v1","repository_id":"{source}","artifact_sha256":"{}"}}"#,
            "0".repeat(64)
        ),
    )
    .unwrap();
    store
}

fn load(store: &Path) -> LoadedPlugin {
    let package = PluginPackage::open(store.join("packages").join(ID).join(VERSION)).unwrap();
    // SAFETY: the component is the fixture above or the portable gain, and
    // the native build is this repository's own, or not a library at all.
    unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap()
}

#[test]
fn a_native_build_that_will_not_load_leaves_the_component() {
    let component = wat::parse_str(QUIET).unwrap();
    for source in ["official", "local"] {
        let store = install(source, &component, b"not a library");
        let plugin = load(&store);
        assert!(!plugin.runs_native_build(), "{source}");
        let mut instance = plugin.create_instance().unwrap();
        instance.activate(48_000.0, 64, 2, 2).unwrap();
        let mut output = vec![1.0_f32; 128];
        instance
            .process_interleaved(&[0.0; 128], &mut output, 64, 2, 2, &[], &[])
            .unwrap();
        assert!(output.iter().all(|sample| *sample == 0.0));
        fs::remove_dir_all(store).ok();
    }
}

/// Needs the portable gain built both ways:
///
///     cargo build -p rackforge-gain-portable
///     cargo build --release --target wasm32-unknown-unknown -p rackforge-gain-portable
#[test]
#[ignore = "requires the native and wasm32 builds of rackforge-gain-portable"]
fn an_official_package_runs_its_native_build_and_sounds_the_same() {
    let target = repository().join("target");
    let component =
        fs::read(target.join("wasm32-unknown-unknown/release/rackforge_gain_portable.wasm"))
            .expect("build the component first");
    let native = fs::read(target.join("debug").join(format!(
        "{}rackforge_gain_portable{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )))
    .expect("build the native library first");

    let render = |plugin: &LoadedPlugin| {
        let mut instance = plugin.create_instance().unwrap();
        instance.activate(48_000.0, 256, 2, 2).unwrap();
        instance.set_parameter(0, 0.37).unwrap();
        let input: Vec<f32> = (0..512).map(|i| ((i as f32) * 0.013).sin()).collect();
        let mut output = vec![0.0_f32; 512];
        instance
            .process_interleaved(&input, &mut output, 256, 2, 2, &[], &[])
            .unwrap();
        let state = instance.save_state().unwrap();
        (output, state)
    };

    let official = install("official", &component, &native);
    let native_plugin = load(&official);
    assert!(native_plugin.runs_native_build());

    // A user's copy of the very same package runs sandboxed.
    let local = install("local", &component, &native);
    let component_plugin = load(&local);
    assert!(!component_plugin.runs_native_build());

    let (native_output, native_state) = render(&native_plugin);
    let (component_output, component_state) = render(&component_plugin);
    let bits = |samples: &[f32]| samples.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&native_output), bits(&component_output));
    assert!(native_output.iter().any(|sample| *sample != 0.0));
    assert_eq!(native_state, component_state);

    // And the switch that turns native builds off is honoured.
    // SAFETY: this test's own process, and no other thread reads it here.
    unsafe { std::env::set_var(rackforge_core::NATIVE_BUILDS_ENV, "off") };
    assert!(!load(&official).runs_native_build());
    unsafe { std::env::remove_var(rackforge_core::NATIVE_BUILDS_ENV) };

    drop(native_plugin);
    fs::remove_dir_all(official).ok();
    fs::remove_dir_all(local).ok();
}
