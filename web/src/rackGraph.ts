import type {
  RackDefinition,
  RackGraph,
  RackGraphEdge,
  RackGraphNode,
  RackSlot,
} from "./types";

export const RACK_GRAPH_SCHEMA_VERSION = 2;

export function rackGraphId(prefix: string) {
  return `${prefix}.${crypto.randomUUID().replaceAll("-", "")}`;
}

export function graphFromSlots(slots: RackSlot[]): RackGraph {
  const midiInput: RackGraphNode = {
    id: "input.midi",
    kind: { kind: "midi_input", bus_id: "main" },
    position: { x: 0, y: 0 },
  };
  const audioBuses = [...new Set(slots.map((slot) => slot.audio_output_bus))].sort();
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

export function materializeRackGraph(rack: RackDefinition): RackDefinition {
  return rack.graph
    ? rack
    : {
        ...rack,
        graph: graphFromSlots(rack.slots),
      };
}

export function addSlotToRack(rack: RackDefinition, slot: RackSlot): RackDefinition {
  const current = materializeRackGraph(rack);
  const graph = current.graph!;
  const midiInput = graph.nodes.find((node) => node.kind.kind === "midi_input");
  const audioOutput = graph.nodes.find(
    (node) => node.kind.kind === "audio_output" && node.kind.bus_id === slot.audio_output_bus,
  );
  if (!midiInput || !audioOutput) {
    return {
      ...current,
      slots: [...current.slots, slot],
      graph: graphFromSlots([...current.slots, slot]),
    };
  }
  const nodeId = rackGraphId("plugin");
  const node: RackGraphNode = {
    id: nodeId,
    kind: { kind: "plugin", slot_id: slot.id },
    position: { x: 360, y: current.slots.length * 180 },
  };
  return {
    ...current,
    slots: [...current.slots, slot],
    graph: {
      ...graph,
      nodes: [...graph.nodes, node],
      edges: [
        ...graph.edges,
        {
          id: rackGraphId("edge.midi"),
          signal: "midi",
          source: { node_id: midiInput.id, port_id: "out" },
          target: { node_id: nodeId, port_id: "midi_in" },
        },
        {
          id: rackGraphId("edge.audio"),
          signal: "audio",
          source: { node_id: nodeId, port_id: "audio_out" },
          target: { node_id: audioOutput.id, port_id: "in" },
        },
      ],
    },
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
