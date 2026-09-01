//! Sandboxed host for RackForge `wasm-v1` processors.
//!
//! RackForge runs the same portable component everywhere. Two backends provide
//! that guarantee:
//!
//! * the native backend compiles the component with Wasmtime, which is what
//!   the Windows, Android and Raspberry Pi hosts use;
//! * the browser backend hands the component to the WebAssembly engine that is
//!   already inside the page, which is what the browser host uses.
//!
//! Both expose [`PortableEngine`], [`PortableModule`] and [`PortableInstance`]
//! with the same methods, so callers such as `rackforge-core` never branch on
//! the target.

mod shared;

pub use shared::{
    ABI_VERSION_V1, ABI_VERSION_V1_1, MAX_PARALLEL_UNITS, MidiEvent, MidiEvent2,
    PARALLEL_ABI_VERSION_V1, ParallelBlockPlan, ParallelLayout, ParallelPlanEntry, ParameterEvent,
    RuntimeLimits,
};

#[cfg(not(target_arch = "wasm32"))]
mod native;
pub use native::unload_process_handlers;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{PortableEngine, PortableInstance, PortableModule};

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::{PortableEngine, PortableInstance, PortableModule, export, host as browser_host};
