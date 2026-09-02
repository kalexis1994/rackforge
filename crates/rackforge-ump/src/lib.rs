//! Windows MIDI Services as a MIDI source: the UMP endpoints the service
//! exposes (a keyboard's groups as sources, the app loopbacks), opened
//! through the MIDI 2.0 SDK's COM surface, delivering UMP words with the
//! service's performance-counter timestamp. Shared by the desktop host and
//! the Concert Grand laboratory, which is why it is a crate of its own:
//! `midir`'s WinMM view does not list an endpoint a controller package owns,
//! this does.
#![cfg(windows)]

mod input;
#[allow(
    dead_code,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all
)]
pub mod midi2_sdk;
pub use input::*;
