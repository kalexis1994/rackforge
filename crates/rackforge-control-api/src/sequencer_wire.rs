//! The sequencer wire shapes, written down for the surfaces that build them.
//!
//! Every sequencer instruction a client sends is a [`SequencerCommand`], and
//! that enum carries `deny_unknown_fields`. A surface that spells a field
//! differently, or invents one, does not get a partly-applied command: the
//! host refuses the whole thing. The Web interface declares its own copy of
//! this union in TypeScript, where the types vanish before anything runs, so
//! nothing inside that language can notice the day the two stop matching.
//!
//! What can notice is this: the field names as `serde` actually emits them,
//! recorded per variant, for the other side to be held to. The names come
//! from serialising a sample of each variant rather than from a list kept up
//! to date by hand, so the record cannot drift from the types it describes
//! even by a typo.
//!
//! `fixtures/sequencer-wire-v1.json` is the output and
//! `web/src/sequencerWire.conformance.test.ts` is where the interface answers
//! for it.

use rackforge_performance_api::{
    FollowAction, ParameterLockSpec, PatternDefinition, PatternId, PatternNoteSpec, PatternView,
    TrigCondition,
};

use crate::{
    SequencerCommand, SequencerLaneStatus, SequencerQuantize, SequencerScale, SequencerStatusV1,
};

/// A pattern with every optional field carrying a non-default value, so that
/// nothing is left out by `skip_serializing_if` and the record shows the full
/// shape rather than the shape of a default.
fn sample_pattern() -> PatternDefinition {
    PatternDefinition {
        id: PatternId::new("sample-pattern").expect("a valid sample id"),
        name: "Sample".to_string(),
        length_ticks: 3_840,
        notes: vec![PatternNoteSpec {
            tick: 0,
            duration_ticks: 240,
            key: 36,
            velocity: 100,
            channel: 1,
            probability: 75,
            condition: TrigCondition::Fill,
            locks: vec![ParameterLockSpec {
                parameter: 0,
                value: 0.5,
            }],
        }],
        view: PatternView::Melodic,
        swing_percent: 66,
        root_key: 48,
        follow_after: 2,
        follow_action: FollowAction::NextSlot,
    }
}

/// One of every command, so the fields of each variant are recorded from a
/// real serialisation rather than from a list kept up to date by hand.
fn sample_commands() -> Vec<SequencerCommand> {
    vec![
        SequencerCommand::TransportStart,
        SequencerCommand::TransportStop,
        SequencerCommand::TransportPanic,
        SequencerCommand::SetTempo { bpm: 120.0 },
        SequencerCommand::SetSignature {
            beats_per_bar: 4,
            beat_unit: 4,
        },
        SequencerCommand::QueuePattern {
            lane: 0,
            pattern: sample_pattern(),
            quantize: SequencerQuantize::NextBar,
        },
        SequencerCommand::LoadSlot {
            lane: 0,
            slot: 1,
            pattern: sample_pattern(),
        },
        SequencerCommand::LaunchSlot {
            lane: 0,
            slot: 1,
            quantize: SequencerQuantize::NextBar,
        },
        SequencerCommand::LaunchLane {
            lane: 0,
            quantize: SequencerQuantize::Now,
        },
        SequencerCommand::StopLane {
            lane: 0,
            quantize: SequencerQuantize::NextBeat,
        },
        SequencerCommand::SetLaneMuted {
            lane: 0,
            muted: true,
        },
        SequencerCommand::SetFocusLane { lane: 0 },
        SequencerCommand::SetCapture { lane: 0, on: true },
        SequencerCommand::SetClockOut { on: true },
        SequencerCommand::SetFill { on: true },
        SequencerCommand::SetLaneFollow {
            lane: 0,
            scale: Some(SequencerScale::Dorian),
        },
        SequencerCommand::SetLaneListenChannel {
            lane: 0,
            channel: Some(1),
        },
    ]
}

/// A status with every optional field present, for the same reason.
fn sample_status() -> SequencerStatusV1 {
    SequencerStatusV1 {
        running: true,
        fill: true,
        clock_out: true,
        focus_lane: 1,
        tempo_bpm: 120.0,
        beats_per_bar: 4,
        beat_unit: 4,
        bar: 2,
        beat_in_bar: 3,
        beat_phase: 0.25,
        lanes: vec![SequencerLaneStatus {
            playing: true,
            queued: true,
            stopping: true,
            following: true,
            capturing: true,
            listen_channel: Some(1),
            active_slot: 2,
            slots: vec![Some("A".to_string()), None],
            muted: true,
            pattern_name: Some("Sample".to_string()),
        }],
    }
}

/// The field names `serde` emits for a value, sorted, with the `kind` tag
/// left out because it is recorded beside them instead.
fn wire_fields(value: &serde_json::Value) -> Vec<String> {
    let mut fields: Vec<String> = value
        .as_object()
        .expect("a sequencer wire shape is a JSON object")
        .keys()
        .filter(|key| key.as_str() != "kind")
        .cloned()
        .collect();
    fields.sort();
    fields
}

/// Which of those fields the host actually demands, asked of `serde` rather
/// than read off the attributes by eye: each field is taken out in turn and
/// the value offered back: what fails to deserialise without it is required.
///
/// The distinction is the whole contract. A field the surface invents is
/// refused by `deny_unknown_fields`, and a field it leaves out is refused by
/// the absence of a default — both lose the command entire, and the two
/// mistakes look nothing alike from the side that made them.
fn required_fields<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Vec<String> {
    wire_fields(value)
        .into_iter()
        .filter(|field| {
            let mut probe = value.clone();
            probe
                .as_object_mut()
                .expect("a sequencer wire shape is a JSON object")
                .remove(field);
            serde_json::from_value::<T>(probe).is_err()
        })
        .collect()
}

/// The strings `serde` emits for a list of unit enum values.
fn enum_values<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| {
            serde_json::to_value(value)
                .expect("a unit enum serialises")
                .as_str()
                .expect("a unit enum serialises to a string")
                .to_string()
        })
        .collect()
}

fn json_string_list(values: &[String]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// Renders the sequencer wire shapes from the types that define them.
///
/// `fixtures/sequencer-wire-v1.json` is this output. The test beside it
/// compares them, so the file is a view of these types rather than a second
/// description of them.
pub fn sequencer_wire_document() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"contract\": \"sequencer-wire-v1\",\n");
    out.push_str(
        "  \"generated_by\": \"UPDATE_SEQUENCER_WIRE=1 cargo test -p rackforge-control-api\",\n",
    );

    let commands = sample_commands();
    out.push_str("  \"commands\": [\n");
    for (index, command) in commands.iter().enumerate() {
        let value = serde_json::to_value(command).expect("a command serialises");
        let kind = value["kind"].as_str().expect("every command is tagged");
        out.push_str(&format!(
            "    {{ \"kind\": \"{}\", \"fields\": {}, \"required\": {} }}",
            kind,
            json_string_list(&wire_fields(&value)),
            json_string_list(&required_fields::<SequencerCommand>(&value))
        ));
        if index + 1 == commands.len() {
            out.push('\n');
        } else {
            out.push_str(",\n");
        }
    }
    out.push_str("  ],\n");

    out.push_str("  \"quantize\": ");
    out.push_str(&json_string_list(&enum_values(&[
        SequencerQuantize::Now,
        SequencerQuantize::NextBeat,
        SequencerQuantize::NextBar,
    ])));
    out.push_str(",\n");

    out.push_str("  \"scales\": ");
    out.push_str(&json_string_list(&enum_values(&[
        SequencerScale::Chromatic,
        SequencerScale::Major,
        SequencerScale::Minor,
        SequencerScale::Dorian,
        SequencerScale::Mixolydian,
        SequencerScale::PentatonicMajor,
        SequencerScale::PentatonicMinor,
    ])));
    out.push_str(",\n");

    let status = serde_json::to_value(sample_status()).expect("a status serialises");
    out.push_str("  \"status_fields\": ");
    out.push_str(&json_string_list(&wire_fields(&status)));
    out.push_str(",\n");
    out.push_str("  \"status_required\": ");
    out.push_str(&json_string_list(&required_fields::<SequencerStatusV1>(
        &status,
    )));
    out.push_str(",\n");

    let lane = &status["lanes"][0];
    out.push_str("  \"lane_status_fields\": ");
    out.push_str(&json_string_list(&wire_fields(lane)));
    out.push_str(",\n");
    out.push_str("  \"lane_status_required\": ");
    out.push_str(&json_string_list(&required_fields::<SequencerLaneStatus>(
        lane,
    )));
    out.push_str(",\n");

    let pattern = serde_json::to_value(sample_pattern()).expect("a pattern serialises");
    out.push_str("  \"pattern_fields\": ");
    out.push_str(&json_string_list(&wire_fields(&pattern)));
    out.push_str(",\n");
    out.push_str("  \"pattern_required\": ");
    out.push_str(&json_string_list(&required_fields::<PatternDefinition>(
        &pattern,
    )));
    out.push('\n');

    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_variant_is_sampled_once() {
        // `sample_commands` is a hand-built list and the enum is not, so the
        // two can part company. A variant sampled twice would hide one that
        // is missing, and this is what notices.
        let kinds: std::collections::BTreeSet<String> = sample_commands()
            .iter()
            .map(|command| {
                serde_json::to_value(command).expect("a command serialises")["kind"]
                    .as_str()
                    .expect("every command is tagged")
                    .to_string()
            })
            .collect();
        assert_eq!(
            kinds.len(),
            sample_commands().len(),
            "a command variant is sampled twice"
        );
    }

    #[test]
    fn the_sequencer_wire_document_matches_these_types() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/sequencer-wire-v1.json"
        );
        let expected = sequencer_wire_document();
        if std::env::var("UPDATE_SEQUENCER_WIRE").is_ok() {
            std::fs::write(path, &expected).expect("writing the sequencer wire document");
            return;
        }
        let actual =
            std::fs::read_to_string(path).expect("reading fixtures/sequencer-wire-v1.json");
        assert_eq!(
            actual, expected,
            "fixtures/sequencer-wire-v1.json is out of date; run \
             UPDATE_SEQUENCER_WIRE=1 cargo test -p rackforge-control-api"
        );
    }
}
