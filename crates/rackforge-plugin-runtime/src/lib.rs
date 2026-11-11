//! Sandboxed host for RackForge `wasm-v1` processors.
//!
//! RackForge runs the same portable component everywhere. Three backends
//! provide that guarantee:
//!
//! * the native backend compiles the component with Wasmtime, which is what
//!   the Windows, Linux, Android and Raspberry Pi hosts use;
//! * beside it, a native build of the same processor, when its package
//!   carries one for the platform and the caller trusts it: the same
//!   contract, called through the SDK's portable table instead of a sandbox;
//! * the browser backend hands the component to the WebAssembly engine that is
//!   already inside the page, which is what the browser host uses.
//!
//! All expose [`PortableEngine`], [`PortableModule`] and [`PortableInstance`]
//! with the same methods, so callers such as `rackforge-core` never branch on
//! the target or on how a processor runs.

mod shared;

pub use shared::{
    ABI_VERSION_V1, ABI_VERSION_V1_1, ABI_VERSION_V1_2, MAX_PARALLEL_UNITS, MidiEvent, MidiEvent2,
    PARALLEL_ABI_VERSION_V1, ParallelBlockPlan, ParallelLayout, ParallelPlanEntry, ParameterEvent,
    RuntimeLimits,
};

#[cfg(not(target_arch = "wasm32"))]
mod library;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod optimize;
#[cfg(not(target_arch = "wasm32"))]
mod portable;
#[cfg(not(target_arch = "wasm32"))]
pub use native::unload_process_handlers;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{PortableEngine, component_uses_simd, optimize_component};
#[cfg(not(target_arch = "wasm32"))]
pub use optimize::DISABLE_ENV as OPTIMIZE_DISABLE_ENV;
#[cfg(not(target_arch = "wasm32"))]
pub use portable::{PortableInstance, PortableModule};

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::{PortableEngine, PortableInstance, PortableModule, export, host as browser_host};
