//! `plugins/gain-c/gain.c` is compiled and played here.
//!
//! docs/PLUGIN_ABI.md claims `wasm-v1` is a plain WebAssembly ABI that any
//! language can implement. A claim like that decays the moment nobody checks
//! it: the SDK keeps working because everything in this repository uses it,
//! and the C path would quietly stop compiling the first time an export
//! changed shape. So the reference plugin is built from source by this test,
//! loaded through the same portable loader a real installation uses, and made
//! to render.
//!
//! It skips on a machine with no wasm-capable clang, because a contributor
//! working on the sequencer should not need one. It does **not** skip in CI:
//! `RACKFORGE_REQUIRE_WASM_CC=1` turns the skip into a failure, and the
//! workflow sets it. A test that can silently do nothing is how this
//! repository has been bitten before -- the Concert Grand shipped broken three
//! times behind an `#[ignore]`.

use std::path::{Path, PathBuf};
use std::process::Command;

use rackforge_plugin_runtime::{MidiEvent, PortableEngine, RuntimeLimits};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A clang that can emit `wasm32`, if this machine has one.
///
/// `RACKFORGE_WASM_CC` wins, then whatever is on PATH, then the Android NDK's
/// LLVM -- which is a full toolchain and already present on any machine set up
/// to build the Android edition.
fn wasm_clang() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("RACKFORGE_WASM_CC") {
        let path = PathBuf::from(explicit);
        return emits_wasm32(&path).then_some(path);
    }
    let candidates = [
        PathBuf::from("clang"),
        repository().join(
            "local/android-toolchain/sdk/ndk/27.0.12077973/toolchains/llvm/prebuilt/\
             windows-x86_64/bin/clang.exe",
        ),
        repository().join(
            "local/android-toolchain/sdk/ndk/27.0.12077973/toolchains/llvm/prebuilt/\
             linux-x86_64/bin/clang",
        ),
    ];
    candidates.into_iter().find(|path| emits_wasm32(path))
}

fn emits_wasm32(clang: &Path) -> bool {
    Command::new(clang)
        .arg("--print-targets")
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("wasm32")
        })
}

fn build_reference_plugin(clang: &Path, destination: &Path) {
    let source = repository().join("plugins/gain-c/gain.c");
    assert!(source.is_file(), "missing {}", source.display());
    let output = Command::new(clang)
        .args([
            "--target=wasm32",
            "-nostdlib",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
        ])
        .args(["-Wl,--no-entry", "-Wl,--export-memory"])
        .arg("-o")
        .arg(destination)
        .arg(&source)
        .output()
        .expect("clang could not be run");
    assert!(
        output.status.success(),
        "the C reference plugin did not compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_c_reference_plugin_builds_loads_and_renders() {
    let required = std::env::var_os("RACKFORGE_REQUIRE_WASM_CC").is_some_and(|value| value == "1");
    let Some(clang) = wasm_clang() else {
        assert!(
            !required,
            "RACKFORGE_REQUIRE_WASM_CC=1 but no clang here emits wasm32. \
             Install clang, or point RACKFORGE_WASM_CC at one."
        );
        eprintln!(
            "skipping: no clang that emits wasm32. Install one, or set \
             RACKFORGE_WASM_CC, to check the C reference plugin."
        );
        return;
    };

    let wasm = std::env::temp_dir().join("rackforge-gain-c-reference.wasm");
    build_reference_plugin(&clang, &wasm);
    let bytes = std::fs::read(&wasm).expect("the compiled plugin must be readable");

    let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
    let module = engine
        .compile(&bytes)
        .expect("C without a single import must load as a wasm-v1 plugin");
    let mut instance = module.instantiate().unwrap();
    instance.prepare(48_000.0, 64, 2, 2).unwrap();

    // The host's own parameter path.
    instance.set_parameter(0, 0.5).unwrap();
    let input = [1.0, -1.0, 0.25, -0.25];
    let mut output = [0.0; 4];
    instance
        .process_interleaved(&input, &mut output, 2)
        .unwrap();
    assert_eq!(
        output,
        [0.5, -0.5, 0.125, -0.125],
        "the C plugin must halve its input"
    );

    // And the MIDI buffer, decoded from the packed 64-bit word by the plugin
    // itself: controller 7 on channel 1, at full scale.
    let volume = MidiEvent::new(0, &[0xb0, 7, 127]).unwrap();
    let mut restored = [0.0; 4];
    instance
        .process_interleaved_with_midi(&input, &mut restored, 2, &[volume])
        .unwrap();
    assert_eq!(
        restored, input,
        "controller 7 at 127 must return the gain to unity"
    );
}
