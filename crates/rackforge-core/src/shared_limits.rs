//! The numbers the interface has to know too.
//!
//! A limit the host enforces is useless to a player if the screen in front of
//! them does not know it. So the Web interface carries its own copy of a
//! handful of them — the longest PLAY chain, the width of a beat, how many
//! lanes a part has — and a copy is a thing that drifts. The drift is not
//! loud when it happens: the drawer lets you add a ninth effect and the host
//! quietly refuses the chain, or a schema number moves on one side and a
//! session stops loading with no obvious cause.
//!
//! This is the list, written down once from the crates that own each number,
//! so the interface can be held to it instead of to its own memory.
//! `fixtures/shared-limits-v1.json` is the output and
//! `web/src/sharedLimits.conformance.test.ts` is where the interface answers
//! for it.
//!
//! Adding a limit here is two lines: the entry below, and the constant it is
//! compared against on the other side.

use rackforge_performance_api::{
    MAX_NOTE_LOCKS, MAX_PART_PATTERN_BINDINGS, MAX_PATTERN_NOTES, MAX_PATTERN_TICKS,
    PATTERN_SWING_MAX, PATTERN_SWING_STRAIGHT, PATTERN_TICKS_PER_BEAT, RACK_GRAPH_SCHEMA_VERSION,
};
use rackforge_session_api::{MAX_PLAY_CHAIN_EFFECTS, SESSION_SCHEMA_VERSION};

use crate::default_instrument::DEFAULT_INSTRUMENT_ID;
use crate::host_bridge::HOST_PROTOCOL;
use crate::sequencer::LANE_SLOTS;
use crate::transport::{MAX_TEMPO_BPM, MIN_TEMPO_BPM};

/// One number, the crate that owns it, and the name the interface knows it by.
struct SharedLimit {
    /// The name on both sides. The interface exports its copy under this name.
    name: &'static str,
    /// The crate the value is declared in, so a failure says where to look.
    owner: &'static str,
    /// Already rendered as JSON, because these are not all the same type.
    value: String,
}

fn shared_limits() -> Vec<SharedLimit> {
    vec![
        SharedLimit {
            name: "DEFAULT_INSTRUMENT_ID",
            owner: "rackforge-core",
            value: format!("\"{DEFAULT_INSTRUMENT_ID}\""),
        },
        SharedLimit {
            name: "MAX_PLAY_CHAIN_EFFECTS",
            owner: "rackforge-session-api",
            value: MAX_PLAY_CHAIN_EFFECTS.to_string(),
        },
        SharedLimit {
            name: "SESSION_SCHEMA_VERSION",
            owner: "rackforge-session-api",
            value: SESSION_SCHEMA_VERSION.to_string(),
        },
        SharedLimit {
            name: "RACK_GRAPH_SCHEMA_VERSION",
            owner: "rackforge-performance-api",
            value: RACK_GRAPH_SCHEMA_VERSION.to_string(),
        },
        SharedLimit {
            name: "TICKS_PER_BEAT",
            owner: "rackforge-performance-api",
            value: PATTERN_TICKS_PER_BEAT.to_string(),
        },
        SharedLimit {
            name: "MAX_SEQUENCER_LANES",
            owner: "rackforge-performance-api",
            value: MAX_PART_PATTERN_BINDINGS.to_string(),
        },
        SharedLimit {
            name: "SWING_STRAIGHT",
            owner: "rackforge-performance-api",
            value: PATTERN_SWING_STRAIGHT.to_string(),
        },
        SharedLimit {
            name: "SWING_MAX",
            owner: "rackforge-performance-api",
            value: PATTERN_SWING_MAX.to_string(),
        },
        SharedLimit {
            name: "LANE_SLOTS",
            owner: "rackforge-core",
            value: LANE_SLOTS.to_string(),
        },
        // The surface builds pattern documents and the host compiles them.
        // A document past any of these three is refused with a
        // `PatternError`, which the player sees as a pattern that will not
        // save and no reason why.
        SharedLimit {
            name: "MAX_PATTERN_NOTES",
            owner: "rackforge-performance-api",
            value: MAX_PATTERN_NOTES.to_string(),
        },
        SharedLimit {
            name: "MAX_PATTERN_TICKS",
            owner: "rackforge-performance-api",
            value: MAX_PATTERN_TICKS.to_string(),
        },
        SharedLimit {
            name: "MAX_NOTE_LOCKS",
            owner: "rackforge-performance-api",
            value: MAX_NOTE_LOCKS.to_string(),
        },
        // The tap-tempo fold clamps to these on both sides.
        SharedLimit {
            name: "MIN_TEMPO_BPM",
            owner: "rackforge-core",
            value: MIN_TEMPO_BPM.to_string(),
        },
        SharedLimit {
            name: "MAX_TEMPO_BPM",
            owner: "rackforge-core",
            value: MAX_TEMPO_BPM.to_string(),
        },
        // Stamped on every envelope crossing the native bridge, and checked
        // on arrival: a copy that disagrees is not answered, and the surface
        // sits there looking like it is still loading.
        SharedLimit {
            name: "HOST_PROTOCOL",
            owner: "rackforge-core",
            value: format!("\"{HOST_PROTOCOL}\""),
        },
    ]
}

/// Renders the shared limits from the crates that declare them.
///
/// `fixtures/shared-limits-v1.json` is this function's output. A test below
/// compares them, so the file is a view of the declarations rather than a
/// third copy of the numbers.
pub fn shared_limits_document() -> String {
    let limits = shared_limits();
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"contract\": \"shared-limits-v1\",\n");
    out.push_str("  \"generated_by\": \"UPDATE_SHARED_LIMITS=1 cargo test -p rackforge-core\",\n");
    out.push_str("  \"limits\": {\n");
    for (index, limit) in limits.iter().enumerate() {
        out.push_str(&format!(
            "    \"{}\": {{ \"value\": {}, \"owner\": \"{}\" }}",
            limit.name, limit.value, limit.owner
        ));
        if index + 1 == limits.len() {
            out.push('\n');
        } else {
            out.push_str(",\n");
        }
    }
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_limit_is_named_once() {
        let limits = shared_limits();
        let mut names: Vec<&str> = limits.iter().map(|limit| limit.name).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "a limit is listed twice");
    }

    /// The Android activity, read as text.
    ///
    /// It is Java, so nothing in this workspace compiles against it and no
    /// test in CI runs it — the Android job builds an APK and stops there.
    /// What can still be done is read it, which is the same thing the Web
    /// conformance tests do to their own source for the same reason.
    fn android_activity() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/rackforge-android/app/src/main/java/org/rackforge/android/MainActivity.java"
        );
        std::fs::read_to_string(path).expect("reading the Android activity")
    }

    /// Every `static final` constant the activity declares, by name, with its
    /// literal exactly as written.
    fn android_constants(source: &str) -> Vec<(String, String)> {
        source
            .lines()
            .filter_map(|line| {
                let after = line.trim().split_once("static final ")?.1;
                let (_type, rest) = after.split_once(' ')?;
                let (name, value) = rest.split_once(" = ")?;
                let value = value.trim().strip_suffix(';')?;
                Some((name.trim().to_string(), value.trim().to_string()))
            })
            .collect()
    }

    #[test]
    fn the_android_host_agrees_on_the_limits_it_declares() {
        // Android is a third language on the same wire, carrying its own copy
        // of some of these, and the record could not see it: the Web test
        // reads the Web source, and nothing read this one. Java writes a
        // string constant as `"text"` and a number as bare digits, which is
        // exactly how the record renders them, so the two compare as written.
        let source = android_activity();
        let declared = android_constants(&source);
        let limits = shared_limits();
        let mut checked = 0;
        for limit in &limits {
            for (name, value) in &declared {
                if name != limit.name {
                    continue;
                }
                assert_eq!(
                    value, &limit.value,
                    "the Android host declares {} as {} while {} declares {}",
                    limit.name, value, limit.owner, limit.value
                );
                checked += 1;
            }
        }
        // Silence is not agreement. If the activity moves, is renamed, or
        // stops spelling its constants this way, every comparison above
        // quietly stops happening and this test passes having read nothing.
        assert!(
            checked >= 2,
            "found only {checked} shared limits in the Android activity; \
             the reader has probably stopped matching how it declares them"
        );
    }

    #[test]
    fn the_shared_limits_match_these_declarations() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/shared-limits-v1.json"
        );
        let expected = shared_limits_document();
        if std::env::var("UPDATE_SHARED_LIMITS").is_ok() {
            std::fs::write(path, &expected).expect("writing the shared limits");
            return;
        }
        // Compared without regard to line endings. The record is stored with
        // newlines, and `text=auto` hands a Windows checkout the same bytes
        // with carriage returns in them, so a contributor there would be told
        // the record is out of date by a difference nobody made and
        // regenerating cannot fix.
        let actual = std::fs::read_to_string(path)
            .expect("reading fixtures/shared-limits-v1.json")
            .replace('\r', "");
        assert_eq!(
            actual, expected,
            "fixtures/shared-limits-v1.json is out of date; run \
             UPDATE_SHARED_LIMITS=1 cargo test -p rackforge-core"
        );
    }
}
