import type {
  RackDefinition,
  RackGraph,
  RackGraphEdge,
  RackGraphNode,
  RackGraphPosition,
  RackMidiTransform,
  RackSlot,
  SongPart,
  SongPartGraph,
} from "./types";
import type { RackPluginRole } from "./rackPluginSelection";
import { scopedId } from "./ids";

export const RACK_GRAPH_SCHEMA_VERSION = 2;
const RACK_GRAPH_COORDINATE_LIMIT = 1_000_000;

export function normalizeRackGraphPosition(
  position: RackGraphPosition,
): RackGraphPosition {
  const coordinate = (value: number) => Math.max(
    -RACK_GRAPH_COORDINATE_LIMIT,
    Math.min(RACK_GRAPH_COORDINATE_LIMIT, Math.round(value)),
  );
  return { x: coordinate(position.x), y: coordinate(position.y) };
}

export function normalizeRackGraphGeometry(
  rack: RackDefinition,
): RackDefinition {
  const current = materializeRackGraph(rack);
  return {
    ...current,
    graph: {
      ...current.graph!,
      nodes: current.graph!.nodes.map((node) => ({
        ...node,
        position: normalizeRackGraphPosition(node.position),
      })),
      labels: (current.graph!.labels ?? []).map((label) => ({
        ...label,
        position: normalizeRackGraphPosition(label.position),
        width: Math.round(label.width),
        height: Math.round(label.height),
      })),
    },
  };
}

export function rackGraphId(prefix: string) {
  return scopedId(prefix);
}

export function midiTransformFromSlot(slot: RackSlot): RackMidiTransform {
  return {
    source_channels: slot.midi_input_channel ? [slot.midi_input_channel] : [],
    note_low: slot.midi_note_low,
    note_high: slot.midi_note_high,
    transpose: slot.midi_transpose,
    notes_only: false,
    velocity_input_low: 0,
    velocity_input_high: 127,
    velocity_output_low: 0,
    velocity_output_high: 127,
  };
}

/**
 * Builds a graph for a Rack that has none.
 *
 * Every Slot is wired for MIDI here, because a Rack old enough to be stored as
 * a bare Slot list is older than effect Slots are: a `RackSlot` records which
 * plugin it holds but not what that plugin does, and only the installed catalog
 * knows. Racks saved since carry their own graph, so this path never sees one.
 */
export function graphFromSlots(slots: RackSlot[]): RackGraph {
  const midiInput: RackGraphNode = {
    id: "input.midi",
    kind: { kind: "midi_input", bus_id: "main" },
    position: { x: 0, y: 0 },
  };
  const audioBuses = [
    ...new Set(["main", ...slots.map((slot) => slot.audio_output_bus)]),
  ].sort();
  const midiBuses = [
    ...new Set(
      slots.flatMap((slot) =>
        slot.midi_output.kind === "bus" ? [slot.midi_output.bus_id] : [],
      ),
    ),
  ].sort();
  const audioOutputs = new Map<string, RackGraphNode>();
  const midiOutputs = new Map<string, RackGraphNode>();
  const nodes: RackGraphNode[] = [midiInput];

  audioBuses.forEach((busId, index) => {
    const node: RackGraphNode = {
      id: `output.audio.${index.toString().padStart(2, "0")}`,
      kind: { kind: "audio_output", bus_id: busId },
      position: { x: 720, y: index * 180 },
    };
    nodes.push(node);
    audioOutputs.set(busId, node);
  });
  midiBuses.forEach((busId, index) => {
    const node: RackGraphNode = {
      id: `output.midi.${index.toString().padStart(2, "0")}`,
      kind: { kind: "midi_output", bus_id: busId },
      position: { x: 720, y: 360 + index * 180 },
    };
    nodes.push(node);
    midiOutputs.set(busId, node);
  });

  const edges: RackGraphEdge[] = [];
  slots.forEach((slot, index) => {
    const number = (index + 1).toString().padStart(2, "0");
    const plugin: RackGraphNode = {
      id: `plugin.${number}`,
      kind: { kind: "plugin", slot_id: slot.id },
      position: { x: 360, y: index * 180 },
    };
    nodes.push(plugin);
    edges.push(
      {
        id: `midi.${number}`,
        signal: "midi",
        source: { node_id: midiInput.id, port_id: "out" },
        target: { node_id: plugin.id, port_id: "midi_in" },
        midi_transform: midiTransformFromSlot(slot),
      },
      {
        id: `audio.${number}`,
        signal: "audio",
        source: { node_id: plugin.id, port_id: "audio_out" },
        target: {
          node_id: audioOutputs.get(slot.audio_output_bus)?.id ?? "output.audio.00",
          port_id: "in",
        },
      },
    );
    if (slot.midi_output.kind === "bus") {
      edges.push({
        id: `midi-output.${number}`,
        signal: "midi",
        source: { node_id: plugin.id, port_id: "midi_out" },
        target: {
          node_id: midiOutputs.get(slot.midi_output.bus_id)?.id ?? "output.midi.00",
          port_id: "in",
        },
      });
    }
  });
  return {
    schema_version: RACK_GRAPH_SCHEMA_VERSION,
    nodes,
    edges,
    labels: [],
  };
}

export function graphFromRackReference(rackId: string): RackGraph {
  return {
    schema_version: RACK_GRAPH_SCHEMA_VERSION,
    nodes: [
      {
        id: "input.midi",
        kind: { kind: "midi_input", bus_id: "main" },
        position: { x: 0, y: 0 },
      },
      {
        id: "rack.01",
        kind: { kind: "rack", rack_id: rackId },
        position: { x: 360, y: 0 },
      },
      {
        id: "output.audio.00",
        kind: { kind: "audio_output", bus_id: "main" },
        position: { x: 720, y: 0 },
      },
    ],
    edges: [
      {
        id: "midi.01",
        signal: "midi",
        source: { node_id: "input.midi", port_id: "out" },
        target: { node_id: "rack.01", port_id: "midi_in" },
        midi_transform: {
          source_channels: [],
          note_low: 0,
          note_high: 127,
          transpose: 0,
          notes_only: false,
          velocity_input_low: 0,
          velocity_input_high: 127,
          velocity_output_low: 0,
          velocity_output_high: 127,
        },
      },
      {
        id: "audio.01",
        signal: "audio",
        source: { node_id: "rack.01", port_id: "audio_out" },
        target: { node_id: "output.audio.00", port_id: "in" },
      },
    ],
    labels: [],
  };
}

export function songPartAsRack(part: SongPart): RackDefinition {
  const content = part.content ?? {
    slots: [],
    graph: graphFromRackReference(part.rack_id),
  };
  return {
    schema_version: 1,
    id: part.id,
    name: part.name,
    enabled: true,
    keyboard_parts: content.keyboard_parts,
    slots: content.slots,
    graph: content.graph,
  };
}

export function songPartGraphFromRack(rack: RackDefinition): SongPartGraph {
  const materialized = normalizeRackGraphGeometry(rack);
  return {
    keyboard_parts: materialized.keyboard_parts,
    slots: materialized.slots,
    graph: materialized.graph!,
  };
}

export function materializeRackGraph(rack: RackDefinition): RackDefinition {
  return rack.graph
    ? rack
    : {
        ...rack,
        graph: graphFromSlots(rack.slots),
      };
}

/**
 * Adds a Slot and wires its node up.
 *
 * An instrument is fed notes: the MIDI input drives it and its audio leaves for
 * the Slot's output bus. An effect is fed sound, so it is wired from the
 * hardware audio input instead — created here if the Rack has none, because a
 * pedalboard whose input is not connected to anything is not a working Rack,
 * and asking someone to draw that cable before they can hear the plugin they
 * just added is asking them to guess.
 *
 * Either way the Slot leaves by the same route, so an effect can be dropped in
 * front of the output and played immediately, or re-patched behind an
 * instrument afterwards.
 */
export function addSlotToRack(
  rack: RackDefinition,
  slot: RackSlot,
  position?: RackGraphPosition,
  role: RackPluginRole = "instrument",
): RackDefinition {
  const current = materializeRackGraph(rack);
  const graph = current.graph!;
  const nodePosition = normalizeRackGraphPosition(
    position ?? { x: 360, y: current.slots.length * 180 },
  );
  const midiInput = graph.nodes.find((node) => node.kind.kind === "midi_input");
  const audioOutput = graph.nodes.find(
    (node) => node.kind.kind === "audio_output" && node.kind.bus_id === slot.audio_output_bus,
  );
  const source = role === "effect" ? audioOutput : midiInput && audioOutput;
  if (!source || !audioOutput) {
    const nextGraph = graphFromSlots([...current.slots, slot]);
    return {
      ...current,
      slots: [...current.slots, slot],
      graph: {
        ...nextGraph,
        nodes: nextGraph.nodes.map((node) =>
          node.kind.kind === "plugin" && node.kind.slot_id === slot.id
            ? { ...node, position: nodePosition }
            : node,
        ),
      },
    };
  }
  const nodeId = rackGraphId("plugin");
  const node: RackGraphNode = {
    id: nodeId,
    kind: { kind: "plugin", slot_id: slot.id },
    position: nodePosition,
  };
  const nodes = [...graph.nodes, node];
  const edges = [...graph.edges];

  if (role === "effect") {
    let audioInput = graph.nodes.find(
      (candidate) => candidate.kind.kind === "audio_input" && candidate.kind.bus_id === "main",
    );
    if (!audioInput) {
      audioInput = {
        id: rackGraphId("input.audio"),
        kind: { kind: "audio_input", bus_id: "main" },
        position: normalizeRackGraphPosition({
          x: nodePosition.x - 360,
          y: nodePosition.y,
        }),
      };
      nodes.push(audioInput);
    }
    edges.push({
      id: rackGraphId("edge.audio-in"),
      signal: "audio",
      source: { node_id: audioInput.id, port_id: "out" },
      target: { node_id: nodeId, port_id: "audio_in" },
    });
  } else {
    edges.push({
      id: rackGraphId("edge.midi"),
      signal: "midi",
      source: { node_id: midiInput!.id, port_id: "out" },
      target: { node_id: nodeId, port_id: "midi_in" },
      midi_transform: midiTransformFromSlot(slot),
    });
  }

  edges.push({
    id: rackGraphId("edge.audio"),
    signal: "audio",
    source: { node_id: nodeId, port_id: "audio_out" },
    target: { node_id: audioOutput.id, port_id: "in" },
  });

  return {
    ...current,
    slots: [...current.slots, slot],
    graph: { ...graph, nodes, edges },
  };
}

export function removeSlotFromRack(rack: RackDefinition, slotId: string): RackDefinition {
  const current = materializeRackGraph(rack);
  const graph = current.graph!;
  const nodeIds = new Set(
    graph.nodes
      .filter((node) => node.kind.kind === "plugin" && node.kind.slot_id === slotId)
      .map((node) => node.id),
  );
  return {
    ...current,
    slots: current.slots.filter((slot) => slot.id !== slotId),
    graph: {
      ...graph,
      nodes: graph.nodes.filter((node) => !nodeIds.has(node.id)),
      edges: graph.edges.filter(
        (edge) =>
          !nodeIds.has(edge.source.node_id) && !nodeIds.has(edge.target.node_id),
      ),
    },
  };
}
