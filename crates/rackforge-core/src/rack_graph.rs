use rackforge_performance_api::{
    PerformanceError, PerformanceLibrary, RackDefinition, RackGraphEdge, RackGraphNode,
    RackGraphNodeId, RackGraphNodeKind, RackGraphSignal, RackId, RackKeyboardParts,
    RackMidiTransform, RackSlot,
};
use std::collections::BTreeMap;
use thiserror::Error;

/// A leaf instrument that the current parallel-voice engine can execute.
/// `runtime_slot_id` includes its Rack path, so using one child Rack more than
/// once never aliases audio voices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledRackSlot {
    pub runtime_slot_id: String,
    pub rack_id: RackId,
    pub slot: RackSlot,
    /// Ordered from the outermost graph (for example a Song Part) to the
    /// instrument cable inside the leaf Rack. Keeping the stages separate is
    /// important: channel filters, velocity curves and note ranges are not in
    /// general losslessly composable into one transform.
    pub midi_stages: Vec<CompiledMidiStage>,
    /// Audio sources are mixed before this node is processed. An empty list
    /// denotes an instrument/source plugin. Ordering is topological.
    pub audio_sources: Vec<CompiledAudioSource>,
    /// Whether this node contributes to the Rack's main output in addition to
    /// feeding any downstream effect nodes.
    pub sends_to_main: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledAudioSource {
    HardwareInput { bus_id: String },
    Slot { runtime_slot_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledMidiStage {
    pub transform: RackMidiTransform,
    pub keyboard_parts: Option<RackKeyboardParts>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RackGraphCompileError {
    #[error("invalid performance library: {0}")]
    InvalidLibrary(#[from] PerformanceError),
    #[error("Rack {0} does not exist")]
    MissingRack(RackId),
    #[error("Rack {rack_id} graph node {node_id} is not supported by the current engine: {reason}")]
    UnsupportedNode {
        rack_id: RackId,
        node_id: RackGraphNodeId,
        reason: &'static str,
    },
    #[error("Rack {0} does not contain an enabled instrument path")]
    NoPlayableSlots(RackId),
}

pub fn compile_instrument_rack(
    library: &PerformanceLibrary,
    rack_id: &RackId,
) -> Result<Vec<CompiledRackSlot>, RackGraphCompileError> {
    library.validate()?;
    let rack = library
        .rack(rack_id)
        .ok_or_else(|| RackGraphCompileError::MissingRack(rack_id.clone()))?;
    compile_instrument_definition(library, rack)
}

/// Compiles a playable graph that may be a persisted Rack or the synthetic
/// root produced by a graph-backed Song Part. Child Rack references are always
/// resolved through the authoritative library.
pub fn compile_instrument_definition(
    library: &PerformanceLibrary,
    rack: &RackDefinition,
) -> Result<Vec<CompiledRackSlot>, RackGraphCompileError> {
    library.validate()?;
    rack.validate()?;
    let mut slots = Vec::new();
    compile_rack(library, rack, rack.id.as_str(), &[], &mut slots)?;
    if slots.is_empty() {
        return Err(RackGraphCompileError::NoPlayableSlots(rack.id.clone()));
    }
    Ok(slots)
}

fn compile_rack(
    library: &PerformanceLibrary,
    rack: &RackDefinition,
    path: &str,
    upstream_midi: &[CompiledMidiStage],
    compiled: &mut Vec<CompiledRackSlot>,
) -> Result<(), RackGraphCompileError> {
    let graph = rack.resolved_graph();
    let nodes = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let slots = rack
        .slots
        .iter()
        .map(|slot| (slot.id.as_str(), slot))
        .collect::<BTreeMap<_, _>>();

    compile_plugin_nodes(
        rack,
        path,
        upstream_midi,
        &nodes,
        &slots,
        &graph.edges,
        compiled,
    )?;

    for node in &graph.nodes {
        match &node.kind {
            RackGraphNodeKind::MidiInput { .. }
            | RackGraphNodeKind::AudioInput { .. }
            | RackGraphNodeKind::AudioOutput { .. }
            | RackGraphNodeKind::Plugin { .. } => {}
            RackGraphNodeKind::Rack { rack_id } => {
                let midi_edge = require_parallel_instrument_path(rack, node, &nodes, &graph.edges)?;
                let child = library
                    .rack(rack_id)
                    .expect("validated library references an existing child Rack");
                let child_path = format!("{path}/{}/{}", node.id, child.id);
                let mut child_midi = upstream_midi.to_vec();
                child_midi.push(CompiledMidiStage {
                    transform: midi_edge.midi_transform.clone().unwrap_or_default(),
                    keyboard_parts: rack.keyboard_parts,
                });
                compile_rack(library, child, &child_path, &child_midi, compiled)?;
            }
            RackGraphNodeKind::MidiOutput { .. } => {
                return unsupported(rack, node, "MIDI output graphs are not implemented yet");
            }
        }
    }
    Ok(())
}

fn compile_plugin_nodes(
    rack: &RackDefinition,
    path: &str,
    upstream_midi: &[CompiledMidiStage],
    nodes: &BTreeMap<&str, &RackGraphNode>,
    slots: &BTreeMap<&str, &RackSlot>,
    edges: &[RackGraphEdge],
    compiled: &mut Vec<CompiledRackSlot>,
) -> Result<(), RackGraphCompileError> {
    let mut pending = nodes
        .values()
        .filter(|node| matches!(node.kind, RackGraphNodeKind::Plugin { .. }))
        .copied()
        .collect::<Vec<_>>();
    let mut runtime_ids = BTreeMap::<String, String>::new();

    while !pending.is_empty() {
        let mut progressed = false;
        let mut index = 0;
        while index < pending.len() {
            let node = pending[index];
            let dependencies = edges
                .iter()
                .filter(|edge| {
                    edge.signal == RackGraphSignal::Audio && edge.target.node_id == node.id
                })
                .filter_map(|edge| match &nodes[edge.source.node_id.as_str()].kind {
                    RackGraphNodeKind::Plugin { .. } => Some(edge.source.node_id.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if dependencies
                .iter()
                .any(|dependency| !runtime_ids.contains_key(*dependency))
            {
                index += 1;
                continue;
            }

            let RackGraphNodeKind::Plugin { slot_id } = &node.kind else {
                unreachable!();
            };
            let slot = slots[slot_id.as_str()];
            let runtime_slot_id = format!("{path}/{}", slot.id);
            runtime_ids.insert(node.id.as_str().to_owned(), runtime_slot_id.clone());
            pending.remove(index);
            progressed = true;
            if !slot.enabled {
                continue;
            }

            let mut midi_stages = upstream_midi.to_vec();
            if let Some(midi_edge) = optional_direct_midi_path(rack, node, nodes, edges)? {
                midi_stages.push(CompiledMidiStage {
                    transform: midi_edge
                        .midi_transform
                        .clone()
                        .unwrap_or_else(|| RackMidiTransform::from_slot(slot)),
                    keyboard_parts: rack.keyboard_parts,
                });
            }

            let mut audio_sources = Vec::new();
            for edge in edges.iter().filter(|edge| {
                edge.signal == RackGraphSignal::Audio && edge.target.node_id == node.id
            }) {
                match &nodes[edge.source.node_id.as_str()].kind {
                    RackGraphNodeKind::AudioInput { bus_id } => {
                        audio_sources.push(CompiledAudioSource::HardwareInput {
                            bus_id: bus_id.clone(),
                        });
                    }
                    RackGraphNodeKind::Plugin { .. } => {
                        audio_sources.push(CompiledAudioSource::Slot {
                            runtime_slot_id: runtime_ids[edge.source.node_id.as_str()].clone(),
                        });
                    }
                    _ => {
                        return unsupported(
                            rack,
                            node,
                            "audio input must come from a hardware input or plugin node",
                        );
                    }
                }
            }
            let outgoing = edges
                .iter()
                .filter(|edge| {
                    edge.signal == RackGraphSignal::Audio && edge.source.node_id == node.id
                })
                .collect::<Vec<_>>();
            if outgoing.is_empty()
                || outgoing.iter().any(|edge| {
                    !matches!(
                        nodes[edge.target.node_id.as_str()].kind,
                        RackGraphNodeKind::Plugin { .. } | RackGraphNodeKind::AudioOutput { .. }
                    )
                })
            {
                return unsupported(
                    rack,
                    node,
                    "plugin audio must feed another plugin or an audio output",
                );
            }
            let sends_to_main = outgoing.iter().any(|edge| {
                matches!(
                    &nodes[edge.target.node_id.as_str()].kind,
                    RackGraphNodeKind::AudioOutput { bus_id } if bus_id == "main"
                )
            });
            compiled.push(CompiledRackSlot {
                runtime_slot_id,
                rack_id: rack.id.clone(),
                slot: slot.clone(),
                midi_stages,
                audio_sources,
                sends_to_main,
            });
        }
        if !progressed {
            let node = pending[0];
            return unsupported(rack, node, "audio dependency order could not be resolved");
        }
    }
    Ok(())
}

fn optional_direct_midi_path<'edge>(
    rack: &RackDefinition,
    node: &RackGraphNode,
    nodes: &BTreeMap<&str, &RackGraphNode>,
    edges: &'edge [RackGraphEdge],
) -> Result<Option<&'edge RackGraphEdge>, RackGraphCompileError> {
    let incoming = edges
        .iter()
        .filter(|edge| edge.signal == RackGraphSignal::Midi && edge.target.node_id == node.id)
        .collect::<Vec<_>>();
    match incoming.as_slice() {
        [] => Ok(None),
        [edge]
            if edge.target.port_id == "midi_in"
                && matches!(
                    nodes[edge.source.node_id.as_str()].kind,
                    RackGraphNodeKind::MidiInput { .. }
                ) =>
        {
            Ok(Some(*edge))
        }
        _ => unsupported(
            rack,
            node,
            "plugin nodes support at most one direct MIDI-input connection",
        ),
    }
}

fn require_parallel_instrument_path<'edge>(
    rack: &RackDefinition,
    node: &RackGraphNode,
    nodes: &BTreeMap<&str, &RackGraphNode>,
    edges: &'edge [RackGraphEdge],
) -> Result<&'edge RackGraphEdge, RackGraphCompileError> {
    let incoming = edges
        .iter()
        .filter(|edge| edge.target.node_id == node.id)
        .collect::<Vec<_>>();
    let outgoing = edges
        .iter()
        .filter(|edge| edge.source.node_id == node.id)
        .collect::<Vec<_>>();
    if incoming.len() != 1
        || incoming[0].signal != RackGraphSignal::Midi
        || incoming[0].target.port_id != "midi_in"
        || !matches!(
            nodes[incoming[0].source.node_id.as_str()].kind,
            RackGraphNodeKind::MidiInput { .. }
        )
    {
        return unsupported(
            rack,
            node,
            "instrument nodes need one direct MIDI-input connection",
        );
    }
    if outgoing.len() != 1
        || outgoing[0].signal != RackGraphSignal::Audio
        || outgoing[0].source.port_id != "audio_out"
        || !matches!(
            &nodes[outgoing[0].target.node_id.as_str()].kind,
            RackGraphNodeKind::AudioOutput { bus_id } if bus_id == "main"
        )
    {
        return unsupported(
            rack,
            node,
            "instrument nodes need one direct connection to the main audio output",
        );
    }
    Ok(incoming[0])
}

fn unsupported<T>(
    rack: &RackDefinition,
    node: &RackGraphNode,
    reason: &'static str,
) -> Result<T, RackGraphCompileError> {
    Err(RackGraphCompileError::UnsupportedNode {
        rack_id: rack.id.clone(),
        node_id: node.id.clone(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_performance_api::{
        MidiOutputRoute, PERFORMANCE_SCHEMA_VERSION, RackGraph, RackGraphEdgeId, RackGraphEndpoint,
        RackGraphPosition, RackSlotId,
    };

    fn slot(id: &str) -> RackSlot {
        RackSlot {
            id: RackSlotId::new(id).unwrap(),
            name: id.into(),
            plugin_id: "org.rackforge.instrument".into(),
            state: None,
            legacy_program_id: None,
            enabled: true,
            midi_input_channel: None,
            midi_note_low: 0,
            midi_note_high: 127,
            midi_transpose: 0,
            midi_output: MidiOutputRoute::None,
            audio_output_bus: "main".into(),
            level_per_mille: 1_000,
            pan_per_mille: 0,
        }
    }

    fn rack(id: &str, slot_id: &str) -> RackDefinition {
        let slots = vec![slot(slot_id)];
        RackDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: RackId::new(id).unwrap(),
            name: id.into(),
            enabled: true,
            keyboard_parts: None,
            graph: Some(RackGraph::from_slots(&slots)),
            slots,
        }
    }

    fn library(racks: Vec<RackDefinition>) -> PerformanceLibrary {
        PerformanceLibrary {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            racks,
            songs: Vec::new(),
            setlists: Vec::new(),
            patterns: Vec::new(),
        }
    }

    #[test]
    fn compiles_a_migrated_flat_rack_to_parallel_voice_slots() {
        let mut rack = rack("rack.main", "piano");
        rack.slots[0].midi_input_channel = Some(5);
        rack.slots[0].midi_note_low = 24;
        rack.slots[0].midi_note_high = 96;
        rack.slots[0].midi_transpose = -12;
        rack.graph.as_mut().unwrap().edges[0].midi_transform = None;
        let library = library(vec![rack]);
        let compiled =
            compile_instrument_rack(&library, &RackId::new("rack.main").unwrap()).unwrap();
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].runtime_slot_id, "rack.main/piano");
        assert_eq!(compiled[0].slot.id.as_str(), "piano");
        assert_eq!(compiled[0].midi_stages.len(), 1);
        assert_eq!(
            compiled[0].midi_stages[0].transform.source_channels,
            vec![5]
        );
        assert_eq!(compiled[0].midi_stages[0].transform.note_low, 24);
        assert_eq!(compiled[0].midi_stages[0].transform.note_high, 96);
        assert_eq!(compiled[0].midi_stages[0].transform.transpose, -12);
    }

    #[test]
    fn compiles_midi_connection_settings_instead_of_legacy_slot_settings() {
        let mut rack = rack("rack.main", "piano");
        let transform = RackMidiTransform {
            source_channels: vec![2, 4, 10],
            target_channel: Some(3),
            note_low: 36,
            note_high: 84,
            transpose: 12,
            notes_only: true,
            velocity_input_low: 10,
            velocity_input_high: 120,
            velocity_output_low: 24,
            velocity_output_high: 112,
        };
        rack.graph.as_mut().unwrap().edges[0].midi_transform = Some(transform.clone());

        let compiled =
            compile_instrument_rack(&library(vec![rack]), &RackId::new("rack.main").unwrap())
                .unwrap();

        assert_eq!(compiled[0].midi_stages.len(), 1);
        assert_eq!(compiled[0].midi_stages[0].transform, transform);
    }

    #[test]
    fn compiles_plugin_chains_in_audio_dependency_order() {
        let mut rack = rack("rack.main", "piano");
        rack.slots.push(slot("effect"));
        rack.graph = Some(RackGraph::from_slots(&rack.slots));
        let graph = rack.graph.as_mut().unwrap();
        graph.edges.retain(|edge| {
            !(edge.signal == RackGraphSignal::Audio && edge.source.node_id.as_str() == "plugin.01")
        });
        graph.edges.push(RackGraphEdge {
            id: RackGraphEdgeId::new("audio.chain").unwrap(),
            signal: RackGraphSignal::Audio,
            source: RackGraphEndpoint {
                node_id: RackGraphNodeId::new("plugin.01").unwrap(),
                port_id: "audio_out".into(),
            },
            target: RackGraphEndpoint {
                node_id: RackGraphNodeId::new("plugin.02").unwrap(),
                port_id: "audio_in".into(),
            },
            midi_transform: None,
        });
        let library = library(vec![rack]);
        let compiled =
            compile_instrument_rack(&library, &RackId::new("rack.main").unwrap()).unwrap();
        assert_eq!(compiled.len(), 2);
        assert!(!compiled[0].sends_to_main);
        assert_eq!(
            compiled[1].audio_sources,
            vec![CompiledAudioSource::Slot {
                runtime_slot_id: "rack.main/piano".into()
            }]
        );
        assert!(compiled[1].sends_to_main);
    }

    #[test]
    fn compiles_hardware_audio_input_into_an_effect() {
        let mut rack = rack("rack.guitar", "pedalboard");
        let graph = rack.graph.as_mut().unwrap();
        graph
            .edges
            .retain(|edge| edge.signal != RackGraphSignal::Midi);
        graph.nodes.push(RackGraphNode {
            id: RackGraphNodeId::new("input.audio.00").unwrap(),
            kind: RackGraphNodeKind::AudioInput {
                bus_id: "main".into(),
            },
            position: RackGraphPosition { x: 0, y: 180 },
        });
        graph.edges.push(RackGraphEdge {
            id: RackGraphEdgeId::new("audio.input").unwrap(),
            signal: RackGraphSignal::Audio,
            source: RackGraphEndpoint {
                node_id: RackGraphNodeId::new("input.audio.00").unwrap(),
                port_id: "out".into(),
            },
            target: RackGraphEndpoint {
                node_id: RackGraphNodeId::new("plugin.01").unwrap(),
                port_id: "audio_in".into(),
            },
            midi_transform: None,
        });
        let compiled =
            compile_instrument_rack(&library(vec![rack]), &RackId::new("rack.guitar").unwrap())
                .unwrap();
        assert_eq!(
            compiled[0].audio_sources,
            vec![CompiledAudioSource::HardwareInput {
                bus_id: "main".into()
            }]
        );
        assert!(compiled[0].midi_stages.is_empty());
    }

    #[test]
    fn compiles_an_instrument_and_an_effect_side_by_side() {
        // Exactly the Rack the interface now builds when a player adds an
        // instrument and then an effect: the keyboard drives one Slot, the
        // guitar jack feeds the other, and both arrive at the main output. The
        // port names asserted here are the contract the web editor writes, so
        // this fails if either side renames one.
        let mut rack = rack("rack.stage", "piano");
        rack.slots.push(RackSlot {
            plugin_id: "org.rackforge.rf-rig".into(),
            ..slot("pedalboard")
        });
        let graph = rack.graph.as_mut().unwrap();
        let audio_output = graph
            .nodes
            .iter()
            .find(|node| matches!(&node.kind, RackGraphNodeKind::AudioOutput { bus_id } if bus_id == "main"))
            .unwrap()
            .id
            .clone();
        graph.nodes.extend([
            RackGraphNode {
                id: RackGraphNodeId::new("input.audio.00").unwrap(),
                kind: RackGraphNodeKind::AudioInput {
                    bus_id: "main".into(),
                },
                position: RackGraphPosition { x: 0, y: 180 },
            },
            RackGraphNode {
                id: RackGraphNodeId::new("plugin.02").unwrap(),
                kind: RackGraphNodeKind::Plugin {
                    slot_id: RackSlotId::new("pedalboard").unwrap(),
                },
                position: RackGraphPosition { x: 360, y: 180 },
            },
        ]);
        graph.edges.extend([
            RackGraphEdge {
                id: RackGraphEdgeId::new("audio.in.02").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("input.audio.00").unwrap(),
                    port_id: "out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.02").unwrap(),
                    port_id: "audio_in".into(),
                },
                midi_transform: None,
            },
            RackGraphEdge {
                id: RackGraphEdgeId::new("audio.out.02").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("plugin.02").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: audio_output,
                    port_id: "in".into(),
                },
                midi_transform: None,
            },
        ]);

        let compiled =
            compile_instrument_rack(&library(vec![rack]), &RackId::new("rack.stage").unwrap())
                .unwrap();
        assert_eq!(compiled.len(), 2);

        let piano = compiled
            .iter()
            .find(|one| one.slot.id.as_str() == "piano")
            .unwrap();
        assert_eq!(piano.midi_stages.len(), 1);
        assert!(piano.audio_sources.is_empty());
        assert!(piano.sends_to_main);

        let pedalboard = compiled
            .iter()
            .find(|one| one.slot.id.as_str() == "pedalboard")
            .unwrap();
        assert!(pedalboard.midi_stages.is_empty());
        assert_eq!(
            pedalboard.audio_sources,
            vec![CompiledAudioSource::HardwareInput {
                bus_id: "main".into()
            }]
        );
        assert!(pedalboard.sends_to_main);
    }

    #[test]
    fn flattens_a_child_rack_with_a_unique_runtime_path() {
        let child = rack("rack.child", "strings");
        let mut parent = rack("rack.parent", "piano");
        let graph = parent.graph.as_mut().unwrap();
        graph.nodes.push(RackGraphNode {
            id: RackGraphNodeId::new("rack.layer").unwrap(),
            kind: RackGraphNodeKind::Rack {
                rack_id: child.id.clone(),
            },
            position: RackGraphPosition { x: 540, y: 200 },
        });
        graph.edges.extend([
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.midi").unwrap(),
                signal: RackGraphSignal::Midi,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("input.midi").unwrap(),
                    port_id: "out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "midi_in".into(),
                },
                midi_transform: None,
            },
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.audio").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("output.audio.00").unwrap(),
                    port_id: "in".into(),
                },
                midi_transform: None,
            },
        ]);
        let library = library(vec![parent, child]);
        let compiled =
            compile_instrument_rack(&library, &RackId::new("rack.parent").unwrap()).unwrap();
        assert_eq!(compiled.len(), 2);
        assert_eq!(compiled[0].runtime_slot_id, "rack.parent/piano");
        assert_eq!(
            compiled[1].runtime_slot_id,
            "rack.parent/rack.layer/rack.child/strings"
        );
        assert_eq!(compiled[1].midi_stages.len(), 2);
    }

    #[test]
    fn compiles_a_song_part_root_with_direct_and_reusable_instruments() {
        let child = rack("rack.child", "strings");
        let mut part_root = rack("part.chorus", "lead");
        let part_to_rack_transform = RackMidiTransform {
            source_channels: vec![2, 7],
            target_channel: Some(4),
            note_low: 36,
            note_high: 96,
            transpose: 12,
            ..RackMidiTransform::default()
        };
        let graph = part_root.graph.as_mut().unwrap();
        graph.nodes.push(RackGraphNode {
            id: RackGraphNodeId::new("rack.layer").unwrap(),
            kind: RackGraphNodeKind::Rack {
                rack_id: child.id.clone(),
            },
            position: RackGraphPosition { x: 540, y: 200 },
        });
        graph.edges.extend([
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.midi").unwrap(),
                signal: RackGraphSignal::Midi,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("input.midi").unwrap(),
                    port_id: "out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "midi_in".into(),
                },
                midi_transform: Some(part_to_rack_transform.clone()),
            },
            RackGraphEdge {
                id: RackGraphEdgeId::new("layer.audio").unwrap(),
                signal: RackGraphSignal::Audio,
                source: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("rack.layer").unwrap(),
                    port_id: "audio_out".into(),
                },
                target: RackGraphEndpoint {
                    node_id: RackGraphNodeId::new("output.audio.00").unwrap(),
                    port_id: "in".into(),
                },
                midi_transform: None,
            },
        ]);
        let library = library(vec![child]);

        let compiled = compile_instrument_definition(&library, &part_root).unwrap();

        assert_eq!(compiled.len(), 2);
        assert_eq!(compiled[0].runtime_slot_id, "part.chorus/lead");
        assert_eq!(
            compiled[1].runtime_slot_id,
            "part.chorus/rack.layer/rack.child/strings"
        );
        assert_eq!(compiled[1].midi_stages.len(), 2);
        assert_eq!(compiled[1].midi_stages[0].transform, part_to_rack_transform);
    }
}
