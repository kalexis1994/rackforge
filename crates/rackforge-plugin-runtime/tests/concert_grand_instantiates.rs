//! The packaged Concert Grand must be able to start at all.
//!
//! This exists because it once could not, and nothing said so. The model's
//! voice bank is built on the stack in `ConcertGrand::default()` -- sixteen
//! voices of a hundred and forty-four partials each, plus the room, lid and
//! halo buffers -- and it sat close enough to wasm's shadow stack limit that
//! adding four small resonators per voice pushed it over. Instantiation then
//! traps with "out of bounds memory access" inside `default`, because the
//! shadow stack simply grows down past the start of linear memory.
//!
//! Nothing caught it. The native tests pass, because native stacks are
//! megabytes; the fit renders pass, for the same reason; and the fuel test,
//! which does load the wasm, is `#[ignore]`d. The broken plugin was built,
//! packaged and installed three times before a listening session found it.
//!
//! So this test is deliberately not ignored, and deliberately trivial: it
//! loads the wasm the build produces and asks it to exist. If it ever fails,
//! the fix is stack headroom -- fewer voices, fewer partials, or smaller
//! buffers -- not a smaller feature.
//!
//! It skips quietly when the wasm has not been built, so it costs nothing to
//! anyone who is not building for the browser.

use std::path::PathBuf;

use rackforge_plugin_runtime::{PortableEngine, RuntimeLimits};

fn wasm_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm")
}

#[test]
fn the_packaged_instrument_can_start() {
    let path = wasm_path();
    if !path.is_file() {
        eprintln!(
            "skipping: no wasm at {}. Build it with\n  \
             cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand",
            path.display()
        );
        return;
    }
    let runtime = PortableEngine::new(RuntimeLimits::default()).expect("runtime");
    let module = runtime
        .compile(&std::fs::read(&path).expect("read wasm"))
        .expect("compile");

    let mut instance = module
        .instantiate()
        .expect("the instrument must instantiate; a trap here is stack overflow in default()");
    instance
        .prepare(48_000.0, 512, 0, 2)
        .expect("the instrument must prepare");

    // And it must survive being asked for audio, which is where the voice
    // bank is actually touched.
    let mut output = vec![0.0f32; 512 * 2];
    instance
        .process_interleaved_with_midi(&[], &mut output, 512, &[])
        .expect("the instrument must render a block");
}
