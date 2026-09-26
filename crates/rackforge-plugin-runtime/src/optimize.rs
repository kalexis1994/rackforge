//! Every plugin component through binaryen before Cranelift sees it.
//!
//! Cranelift is built to compile quickly and safely, not to clean up after a
//! code generator: what LLVM leaves in a `.wasm` -- redundant locals, copies,
//! blocks a simplifier would merge -- Cranelift mostly lowers as it finds it.
//! `wasm-opt -O3` does that cleaning at the WebAssembly level, where it does
//! not cost the host a thing at render time. Measured on the Concert Grand
//! (`plugins/concert-grand/examples/wasm-tax.rs`, x86_64): the host's path
//! went from 66 to 72 against the same code compiled natively, with audio
//! identical to the bit.
//!
//! Doing it here rather than asking every author to is the point: a plugin
//! built without it -- or by a toolchain that has never heard of binaryen --
//! gets the same code as one built with it. The package is not touched. The
//! optimised component lives in the host's disposable cache, keyed by the
//! hash of the original, and is made once per plugin version on each machine.
//!
//! Anything that goes wrong leaves the original in place: an optimiser that
//! cannot run, or a result that does not validate, costs speed and never a
//! plugin.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use wasm_opt::{Feature, OptimizationOptions};

/// Part of the cache key, so a change of optimiser or settings makes new
/// entries rather than reusing ones made differently.
const OPTIMIZER_TAG: &str = "binaryen-116-O3-v1";

/// Set to `0` to compile every component exactly as shipped -- for telling
/// apart a plugin's fault from the optimiser's.
pub const DISABLE_ENV: &str = "RACKFORGE_OPTIMIZE_PLUGINS";

pub fn disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| value.trim() == "0")
}

/// Where the optimised form of `bytes` is kept below `cache`.
pub fn cached_path(cache: &Path, bytes: &[u8]) -> PathBuf {
    let mut hash = Sha256::new();
    hash.update(OPTIMIZER_TAG.as_bytes());
    hash.update(bytes);
    let digest = hash.finalize();
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    cache.join("optimized").join(format!("{name}.wasm"))
}

/// Runs binaryen over `bytes`. The caller has already validated them: a
/// module binaryen cannot read may end the process rather than return.
pub fn optimize(bytes: &[u8], scratch: &Path) -> Result<Vec<u8>, String> {
    fs::create_dir_all(scratch).map_err(|error| error.to_string())?;
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos())
    );
    let input = scratch.join(format!("{stamp}.in.wasm"));
    let output = scratch.join(format!("{stamp}.out.wasm"));
    let result = (|| {
        fs::write(&input, bytes).map_err(|error| error.to_string())?;
        OptimizationOptions::new_opt_level_3()
            // Exactly what the engines accept, and nothing that changes
            // arithmetic: no relaxed SIMD, no fast-math.
            .enable_feature(Feature::Simd)
            .enable_feature(Feature::BulkMemory)
            .enable_feature(Feature::TruncSat)
            .enable_feature(Feature::SignExt)
            .enable_feature(Feature::MutableGlobals)
            .enable_feature(Feature::Multivalue)
            .enable_feature(Feature::ReferenceTypes)
            .run(&input, &output)
            .map_err(|error| error.to_string())?;
        fs::read(&output).map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(&input);
    let _ = fs::remove_file(&output);
    result
}

/// Stores an optimised component where [`cached_path`] will find it,
/// atomically, so a reader never sees half a file.
pub fn store(path: &Path, optimized: &[u8]) -> Result<(), String> {
    let directory = path.parent().ok_or("cache path has no directory")?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    fs::write(&partial, optimized).map_err(|error| error.to_string())?;
    fs::rename(&partial, path).map_err(|error| {
        let _ = fs::remove_file(&partial);
        error.to_string()
    })
}
