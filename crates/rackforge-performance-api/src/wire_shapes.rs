//! The performance library shapes, written down for the surface that mirrors
//! them.
//!
//! Nearly every type in this crate is declared a second time in TypeScript,
//! by hand, so the Web interface can build a rack or a song and send it back.
//! They all carry `deny_unknown_fields`. A field the interface invents,
//! misspells, or renames is not a setting that fails to apply — the host
//! refuses the edit, and what the player loses is the work they just did.
//!
//! Nothing here is a sample. Every name below is asked of `serde` by offering
//! it something it cannot possibly accept and reading what it says it
//! expected instead. That matters for more than brevity: a sample only shows
//! the fields a sample happens to set, while the refusal lists the whole
//! accepted set — including the legacy aliases a migration still honours,
//! which is exactly the part nobody remembers to write down.
//!
//! `fixtures/performance-wire-v1.json` is the output and
//! `web/src/performanceWire.conformance.test.ts` is where the interface
//! answers for it.

use serde::de::DeserializeOwned;

/// The names `serde` lists after saying what it expected instead.
fn expected_names(message: &str) -> Vec<String> {
    let Some(tail) = message.split_once("expected ") else {
        return Vec::new();
    };
    tail.1
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|name| name.to_string())
        .collect()
}

/// What a struct accepts, as `deny_unknown_fields` enforces it.
///
/// `None` means the type took the nonsense field without complaint, which
/// means it is not closed and this record cannot describe it.
fn accepted_fields<T: DeserializeOwned>() -> Option<Vec<String>> {
    match serde_json::from_str::<T>("{\"__rackforge_probe__\": 1}") {
        Ok(_) => None,
        Err(error) => {
            let names = expected_names(&error.to_string());
            if names.is_empty() { None } else { Some(names) }
        }
    }
}

/// What an enum accepts, whether it is written as a bare string or carries a
/// `kind` tag. The two spellings are tried in turn because the refusal only
/// names the variants once the shape itself is right.
fn accepted_variants<T: DeserializeOwned>() -> Option<Vec<String>> {
    for payload in [
        "\"__rackforge_probe__\"",
        "{\"kind\": \"__rackforge_probe__\"}",
    ] {
        if let Err(error) = serde_json::from_str::<T>(payload) {
            let message = error.to_string();
            if message.contains("unknown variant") {
                let names = expected_names(&message);
                if !names.is_empty() {
                    return Some(names);
                }
            }
        }
    }
    None
}

fn json_string_list(values: &[String]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// Renders one entry, or a null the test will refuse, so a type that stops
/// being closed is loud rather than absent.
fn entry(name: &str, key: &str, names: Option<Vec<String>>, last: bool) -> String {
    let rendered = match names {
        Some(values) => json_string_list(&values),
        None => "null".to_string(),
    };
    format!(
        "    {{ \"name\": \"{}\", \"{}\": {} }}{}\n",
        name,
        key,
        rendered,
        if last { "" } else { "," }
    )
}

/// Every shape in this crate that the Web interface declares a copy of.
///
/// The list is written out rather than discovered, because "what TypeScript
/// also declares" is not something this side can see. A type mirrored there
/// and missing here is the one gap this mechanism cannot close by itself, so
/// the test on the other side closes it: it fails on an interface it cannot
/// find a record for.
macro_rules! struct_entries {
    ($($shape:ty),* $(,)?) => {{
        let names: Vec<&str> = vec![$(stringify!($shape)),*];
        let fields: Vec<Option<Vec<String>>> = vec![$(accepted_fields::<$shape>()),*];
        (names, fields)
    }};
}

macro_rules! enum_entries {
    ($($shape:ty),* $(,)?) => {{
        let names: Vec<&str> = vec![$(stringify!($shape)),*];
        let variants: Vec<Option<Vec<String>>> = vec![$(accepted_variants::<$shape>()),*];
        (names, variants)
    }};
}

/// Renders the performance wire shapes from the types that define them.
///
/// `fixtures/performance-wire-v1.json` is this output. The test below
/// compares them, so the file is a view of these types rather than a second
/// description of them.
fn performance_wire_document() -> String {
    use crate::*;

    let (struct_names, struct_fields) = struct_entries![
        LivePerformanceState,
        ParameterLockSpec,
        PatternDefinition,
        PatternNoteSpec,
        PerformanceLibrary,
        PerformanceSnapshot,
        RackDefinition,
        RackGraph,
        RackGraphEdge,
        RackGraphEndpoint,
        RackGraphLabel,
        RackGraphNode,
        RackGraphPosition,
        RackKeyboardPart,
        RackKeyboardParts,
        RackMidiTransform,
        RackSlot,
        SequencerTabDefinition,
        SetlistDefinition,
        SetlistEntry,
        SongDefinition,
        SongPart,
        SongPartGraph,
        SongPartPatternBinding,
    ];

    let (enum_names, enum_variants) = enum_entries![
        LiveBrowseMode,
        LiveLocation,
        MidiOutputRoute,
        PerformanceEdit,
        RackGraphLabelTone,
        RackGraphNodeKind,
        RackGraphSignal,
        TrigCondition,
    ];

    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"contract\": \"performance-wire-v1\",\n");
    out.push_str(
        "  \"generated_by\": \"UPDATE_PERFORMANCE_WIRE=1 cargo test -p rackforge-performance-api\",\n",
    );

    out.push_str("  \"shapes\": [\n");
    for (index, name) in struct_names.iter().enumerate() {
        out.push_str(&entry(
            name,
            "fields",
            struct_fields[index].clone(),
            index + 1 == struct_names.len(),
        ));
    }
    out.push_str("  ],\n");

    out.push_str("  \"vocabularies\": [\n");
    for (index, name) in enum_names.iter().enumerate() {
        out.push_str(&entry(
            name,
            "variants",
            enum_variants[index].clone(),
            index + 1 == enum_names.len(),
        ));
    }
    out.push_str("  ]\n");

    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recorded_shape_is_closed() {
        // A `null` in the record means the probe was accepted, which means
        // the type no longer refuses unknown fields. That is a wire this
        // record cannot describe, and a surface could then send anything
        // into it — so it fails here rather than passing quietly there.
        let document = performance_wire_document();
        assert!(
            !document.contains(": null }"),
            "a recorded shape stopped refusing unknown fields:\n{document}"
        );
    }

    #[test]
    fn the_performance_wire_document_matches_these_types() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/performance-wire-v1.json"
        );
        let expected = performance_wire_document();
        if std::env::var("UPDATE_PERFORMANCE_WIRE").is_ok() {
            std::fs::write(path, &expected).expect("writing the performance wire document");
            return;
        }
        // Compared without regard to line endings. The record is stored with
        // newlines, and `text=auto` hands a Windows checkout the same bytes
        // with carriage returns in them, so a contributor there would be told
        // the record is out of date by a difference nobody made and
        // regenerating cannot fix.
        let actual = std::fs::read_to_string(path)
            .expect("reading fixtures/performance-wire-v1.json")
            .replace('\r', "");
        assert_eq!(
            actual, expected,
            "fixtures/performance-wire-v1.json is out of date; run \
             UPDATE_PERFORMANCE_WIRE=1 cargo test -p rackforge-performance-api"
        );
    }
}
