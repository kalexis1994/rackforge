//! The handshake between the RackForge interface and the host shell holding it.
//!
//! On Android and inside the VST3 editor the Web interface runs in a WebView
//! owned by a native host, and the two speak through one bridge:
//! `window.RackForgeNativeHost.postMessage`, carrying JSON envelopes stamped
//! with the protocol below. Every envelope is checked against that stamp and
//! dropped if it does not match, which is the point — a page that is not
//! RackForge, or a RackForge from a different generation, gets no answer.
//!
//! That makes the stamp a thing all three implementations have to spell
//! identically, and until now all three spelled it separately: a constant in
//! `web/src/host.ts`, a constant in the Android activity, and four loose
//! copies of the literal inside the VST3 host. Nothing compared them. A typo
//! in any one of them does not fail a build or log a warning — the messages
//! simply stop being answered, and the surface sits there looking like it is
//! still loading.
//!
//! This is the one declaration. `rackforge-core` holds it because every native
//! shell already depends on the core, and the other two languages are held to
//! it by test: the Web copy through the shared-limits record, and the Android
//! copy by [`shared_limits`](crate::shared_limits), which reads the activity
//! source and refuses a constant that disagrees.

/// The stamp on every envelope crossing the native bridge.
///
/// The `@1` is a generation, not a decoration. Raising it is how a host says
/// it no longer understands the older envelopes, so raise it here and in the
/// same breath as the change that earns it.
pub const HOST_PROTOCOL: &str = "rackforge.host@1";

/// What an envelope can be, in the direction it travels.
///
/// `request` and `file` go from the page to the shell; `response` and `event`
/// come back. All four are spelled in each implementation, so they are named
/// here for the same reason the protocol is.
pub const MESSAGE_KINDS: [&str; 4] = ["request", "response", "event", "file"];

/// Where a shell script writes the protocol it stamps.
///
/// The VST3 host injects its bridge as JavaScript, which is full of the braces
/// `format!` would read as its own. Rather than double every one of them, the
/// script carries this marker and the host replaces it — so the value still
/// comes from [`HOST_PROTOCOL`] and the script stays readable as the script it
/// is.
pub const PROTOCOL_PLACEHOLDER: &str = "__RACKFORGE_HOST_PROTOCOL__";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_protocol_names_a_generation() {
        let (name, generation) = HOST_PROTOCOL
            .split_once('@')
            .expect("the protocol carries a generation after an @");
        assert_eq!(name, "rackforge.host");
        assert!(
            generation.parse::<u32>().is_ok(),
            "the generation is a number, not {generation:?}"
        );
    }

    #[test]
    fn the_message_kinds_are_distinct() {
        let mut kinds = MESSAGE_KINDS;
        kinds.sort_unstable();
        let mut unique = kinds.to_vec();
        unique.dedup();
        assert_eq!(unique.len(), MESSAGE_KINDS.len(), "a kind is named twice");
    }
}
