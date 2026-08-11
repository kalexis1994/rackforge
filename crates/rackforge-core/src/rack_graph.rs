use rackforge_performance_api::{
    PerformanceError, PerformanceLibrary, RackDefinition, RackGraphEdge, RackGraphNode,
    RackGraphNodeId, RackGraphNodeKind, RackGraphSignal, RackId, RackKeyboardParts, RackSlot,
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
    let mut slots = Vec::new();
    compile_rack(library, rack, rack.id.as_str(), &mut slots)?;
    if slots.is_empty() {
        return Err(RackGraphCompileError::NoPlayableSlots(rack.id.clone()));
    }
    Ok(slots)
}

fn compile_rack(
    library: &PerformanceLibrary,
    rack: &RackDefinition,
    path: &str,
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

    for node in &graph.nodes {
        match &node.kind {
            RackGraphNodeKind::MidiInput { .. } | RackGraphNodeKind::AudioOutput { .. } => {}
            RackGraphNodeKind::Plugin { slot_id } => {
                let slot = slots
                    .get(slot_id.as_str())
                    .expect("validated Rack graph references an existing Slot");
                if !slot.enabled {
                    continue;
                }
                require_parallel_instrument_path(rack, node, &nodes, &graph.edges)?;
                compiled.push(CompiledRackSlot {
                    runtime_slot_id: format!("{path}/{}", slot.id),
                    rack_id: rack.id.clone(),
                    slot: (*slot).clone(),
                    keyboard_parts: rack.keyboard_parts,
                });
            }
            RackGraphNodeKind::Rack { rack_id } => {
                require_parallel_instrument_path(rack, node, &nodes, &graph.edges)?;
                let child = library
                    .rack(rack_id)
                    .expect("validated library references an existing child Rack");
                let child_path = format!("{path}/{}/{}", node.id, child.id);
                compile_rack(library, child, &child_path, compiled)?;
            }
            RackGraphNodeKind::AudioInput { .. } => {
                return unsupported(
                    rack,
                    node,
                    "audio-input/effect graphs are not implemented yet",
                );
            }
            RackGraphNodeKind::MidiOutput { .. } => {
                return unsupported(rack, node, "MIDI output graphs are not implemented yet");
            }
        }
    }
    Ok(())
}

fn require_parallel_instrument_path(
    rack: &RackDefinition,
    node: &RackGraphNode,
    nodes: &BTreeMap<&str, &RackGraphNode>,
    edges: &[RackGraphEdge],
) -> Result<(), RackGraphCompileError> {
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
    Ok(())
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
        }
    }

    #[test]
    fn compiles_a_migrated_flat_rack_to_parallel_voice_slots() {
        let library = library(vec![rack("rack.main", "piano")]);
        let compiled =
            compile_instrument_rack(&library, &RackId::new("rack.main").unwrap()).unwrap();
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].runtime_slot_id, "rack.main/piano");
        assert_eq!(compiled[0].slot.id.as_str(), "piano");
    }

    #[test]
    fn rejects_plugin_chains_until_the_effect_engine_exists() {
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
        });
        let library = library(vec![rack]);
        assert!(matches!(
            compile_instrument_rack(&library, &RackId::new("rack.main").unwrap()),
            Err(RackGraphCompileError::UnsupportedNode { .. })
        ));
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
    }
}
