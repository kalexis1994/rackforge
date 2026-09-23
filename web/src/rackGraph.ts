import type {
  RackDefinition,
  RackGraph,
  RackGraphEdge,
  RackGraphNode,
  RackGraphPosition,
  RackGraphSignal,
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

/*
 * ------------------------------------------------------------ the rules ---
 *
 * What a Rack graph may contain, stated once, for the editor to hold to while
 * a cable is still being dragged and for every function here that rewires
 * one. The engine compiles only graphs of this shape (crates/rackforge-core/
 * src/rack_graph.rs); a cable the editor let through and the engine refused
 * surfaced as a preview error, long after the gesture that caused it.
 *
 *   MIDI   leaves the MIDI input and enters a plugin or a child Rack. A node
 *          takes one MIDI cable; the MIDI input may drive any number.
 *   Audio  leaves the audio input, a plugin or a child Rack, and enters a
 *          plugin or an audio output. Every audio output port feeds exactly
 *          one place: a sound goes one way, and heard twice it is doubled.
 *          Any number may enter one place, which mixes them.
 *   A child Rack is played whole: its audio goes straight to the main output
 *          and it takes no audio in.
 *   The audio input alone goes nowhere -- a cable from it straight to an
 *          output would carry nothing; it needs a plugin between.
 *   No loops.
 */

export interface RackConnection {
  signal: RackGraphSignal;
  source: { node_id: string; port_id: string };
  target: { node_id: string; port_id: string };
}

function isMainOutput(node: RackGraphNode) {
  return node.kind.kind === "audio_output" && node.kind.bus_id === "main";
}

function sourcePortAllowed(node: RackGraphNode, signal: RackGraphSignal, portId: string) {
  switch (node.kind.kind) {
    case "midi_input":
      return signal === "midi" && portId === "out";
    case "audio_input":
      return signal === "audio" && portId === "out";
    case "plugin":
    case "rack":
      return signal === "audio" && portId === "audio_out";
    default:
      return false;
  }
}

function targetPortAllowed(node: RackGraphNode, signal: RackGraphSignal, portId: string) {
  switch (node.kind.kind) {
    case "plugin":
      return (signal === "midi" && portId === "midi_in")
        || (signal === "audio" && portId === "audio_in");
    case "rack":
      return signal === "midi" && portId === "midi_in";
    case "audio_output":
      return signal === "audio" && portId === "in";
    default:
      return false;
  }
}

/** The edges a connection displaces: the source's audio cable, which it may
 *  only have one of, and the target's MIDI cable, likewise. */
function displacedBy(graph: RackGraph, connection: RackConnection) {
  return graph.edges.filter((edge) =>
    edge.signal === connection.signal
    && (connection.signal === "audio"
      ? edge.source.node_id === connection.source.node_id
        && edge.source.port_id === connection.source.port_id
      : edge.target.node_id === connection.target.node_id
        && edge.target.port_id === connection.target.port_id),
  );
}

function reaches(edges: RackGraphEdge[], from: string, to: string) {
  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    const list = outgoing.get(edge.source.node_id) ?? [];
    list.push(edge.target.node_id);
    outgoing.set(edge.source.node_id, list);
  }
  const seen = new Set<string>();
  const stack = [from];
  while (stack.length > 0) {
    const id = stack.pop()!;
    if (id === to) return true;
    if (seen.has(id)) continue;
    seen.add(id);
    stack.push(...(outgoing.get(id) ?? []));
  }
  return false;
}

/**
 * Why a cable may not be drawn, or null when it may. A cable that is already
 * there is not a problem; drawing it again changes nothing.
 */
export function rackConnectionProblem(
  graph: RackGraph,
  connection: RackConnection,
): string | null {
  const { signal, source, target } = connection;
  if (source.node_id === target.node_id) return "A node cannot feed itself.";
  const from = graph.nodes.find((node) => node.id === source.node_id);
  const to = graph.nodes.find((node) => node.id === target.node_id);
  if (!from || !to) return "Both ends need a node.";
  if (!sourcePortAllowed(from, signal, source.port_id)) {
    return signal === "midi"
      ? "MIDI comes from the MIDI input."
      : "That port does not send audio.";
  }
  if (!targetPortAllowed(to, signal, target.port_id)) {
    return to.kind.kind === "rack" && signal === "audio"
      ? "A child Rack takes no audio in."
      : "That port does not take this signal.";
  }
  if (from.kind.kind === "rack" && !isMainOutput(to)) {
    return "A child Rack goes straight to the main output.";
  }
  if (from.kind.kind === "audio_input" && to.kind.kind === "audio_output") {
    return "The audio input needs a plugin between it and an output.";
  }
  const remaining = graph.edges.filter(
    (edge) => !displacedBy(graph, connection).includes(edge),
  );
  if (reaches(remaining, target.node_id, source.node_id)) {
    return "That cable would make a loop.";
  }
  return null;
}

function connectionExists(graph: RackGraph, connection: RackConnection) {
  return graph.edges.some((edge) =>
    edge.signal === connection.signal
    && edge.source.node_id === connection.source.node_id
    && edge.source.port_id === connection.source.port_id
    && edge.target.node_id === connection.target.node_id
    && edge.target.port_id === connection.target.port_id);
}

/**
 * Draws a cable, if the rules allow it. An audio output that was already
 * patched somewhere is re-patched -- the new cable replaces the old, as
 * moving a plug does -- and a MIDI input already fed is fed by the new one.
 * A cable the rules refuse leaves the graph as it was.
 */
export function connectRackGraph(
  graph: RackGraph,
  connection: RackConnection,
  edgeId: string = rackGraphId("edge"),
): RackGraph {
  if (connectionExists(graph, connection)) return graph;
  if (rackConnectionProblem(graph, connection) !== null) return graph;
  const displaced = new Set(displacedBy(graph, connection).map((edge) => edge.id));
  return {
    ...graph,
    edges: [
      ...graph.edges.filter((edge) => !displaced.has(edge.id)),
      {
        id: edgeId,
        signal: connection.signal,
        source: { ...connection.source },
        target: { ...connection.target },
      },
    ],
  };
}

export interface RackGraphProblem {
  nodeId: string;
  message: string;
}

/**
 * What in a graph the engine will refuse, node by node: Racks saved before
 * the editor held to the rules, or written elsewhere, can break them, and the
 * editor says so where the node is rather than leaving it to a preview error.
 */
export function rackGraphProblems(graph: RackGraph): RackGraphProblem[] {
  const problems: RackGraphProblem[] = [];
  const nodes = new Map(graph.nodes.map((node) => [node.id, node]));
  for (const edge of graph.edges) {
    const source = nodes.get(edge.source.node_id);
    const target = nodes.get(edge.target.node_id);
    if (!source || !target) continue;
    const others = { ...graph, edges: graph.edges.filter((one) => one !== edge) };
    const problem = rackConnectionProblem(others, {
      signal: edge.signal,
      source: edge.source,
      target: edge.target,
    });
    if (problem && problem !== "That cable would make a loop.") {
      problems.push({ nodeId: source.id, message: problem });
    }
  }
  for (const node of graph.nodes) {
    if (node.kind.kind !== "plugin" && node.kind.kind !== "rack" && node.kind.kind !== "audio_input") {
      continue;
    }
    const audioOut = graph.edges.filter(
      (edge) => edge.signal === "audio" && edge.source.node_id === node.id,
    );
    if (audioOut.length > 1) {
      problems.push({
        nodeId: node.id,
        message: `Its audio goes to ${audioOut.length} places; it may go to one.`,
      });
    }
    if (node.kind.kind === "audio_input") continue;
    if (audioOut.length === 0) {
      problems.push({
        nodeId: node.id,
        message: "It is not heard: connect its audio to an effect or an output.",
      });
    }
    const midiIn = graph.edges.filter(
      (edge) => edge.signal === "midi" && edge.target.node_id === node.id,
    );
    if (midiIn.length > 1) {
      problems.push({ nodeId: node.id, message: "It takes more than one MIDI cable." });
    }
    if (node.kind.kind === "rack" && midiIn.length === 0) {
      problems.push({ nodeId: node.id, message: "It needs a MIDI cable from the MIDI input." });
    }
  }
  return problems;
}

/**
 * Makes the graph's MIDI input and an audio output for `busId` exist, the
 * ends every plugin node is wired between. Missing ones are created beside
 * `near`, rather than the Rack being rebuilt from its Slot list -- which
 * dropped every cable and position, and wired effects as instruments.
 */
function withEnds(graph: RackGraph, busId: string, near: RackGraphPosition) {
  const nodes = [...graph.nodes];
  let midiInput = nodes.find((node) => node.kind.kind === "midi_input");
  if (!midiInput) {
    midiInput = {
      id: rackGraphId("input.midi"),
      kind: { kind: "midi_input", bus_id: "main" },
      position: normalizeRackGraphPosition({ x: near.x - 360, y: near.y }),
    };
    nodes.push(midiInput);
  }
  let audioOutput = nodes.find(
    (node) => node.kind.kind === "audio_output" && node.kind.bus_id === busId,
  );
  if (!audioOutput) {
    audioOutput = {
      id: rackGraphId("output.audio"),
      kind: { kind: "audio_output", bus_id: busId },
      position: normalizeRackGraphPosition({ x: near.x + 360, y: near.y }),
    };
    nodes.push(audioOutput);
  }
  return { graph: { ...graph, nodes }, midiInput, audioOutput };
}

function nodeKind(graph: RackGraph, id: string) {
  return graph.nodes.find((node) => node.id === id)?.kind.kind;
}

/**
 * Adds a Slot and wires its node in where it will be heard.
 *
 * An instrument is fed notes from the MIDI input. Its audio goes where the
 * Rack's other instruments go: if they all feed one effect chain, the new one
 * joins it at the head, so it is heard through the same effects; otherwise
 * it goes to its output bus.
 *
 * An effect is inserted between whatever reaches the output and the output:
 * every plugin feeding the output bus now feeds the effect, and the effect
 * feeds the output -- one instrument or several, and an effect added after
 * another comes after it. Child Racks keep their own cable to the main
 * output, which is how the engine plays them. Only when nothing plays into
 * the output yet is the effect fed from the hardware audio input -- created
 * if the Rack has none -- as a pedalboard is.
 */
export function addSlotToRack(
  rack: RackDefinition,
  slot: RackSlot,
  position?: RackGraphPosition,
  role: RackPluginRole = "instrument",
): RackDefinition {
  const current = materializeRackGraph(rack);
  const provisional = normalizeRackGraphPosition(
    position ?? { x: 360, y: current.slots.length * 180 },
  );
  const ends = withEnds(current.graph!, slot.audio_output_bus, provisional);
  const { midiInput, audioOutput } = ends;
  let graph = ends.graph;
  const placed = position ? null : placeNewNode(graph, role, midiInput, audioOutput);
  const nodePosition = placed?.position ?? provisional;
  if (placed?.outputPosition) {
    graph = {
      ...graph,
      nodes: graph.nodes.map((node) =>
        node.id === audioOutput.id ? { ...node, position: placed.outputPosition! } : node),
    };
  }
  const nodeId = rackGraphId("plugin");
  graph = {
    ...graph,
    nodes: [
      ...graph.nodes,
      { id: nodeId, kind: { kind: "plugin", slot_id: slot.id }, position: nodePosition },
    ],
  };

  if (role === "effect") {
    const feeders = graph.edges.filter((edge) =>
      edge.signal === "audio"
      && edge.target.node_id === audioOutput.id
      && nodeKind(graph, edge.source.node_id) === "plugin");
    if (feeders.length > 0) {
      for (const feeder of feeders) {
        graph = connectRackGraph(graph, {
          signal: "audio",
          source: feeder.source,
          target: { node_id: nodeId, port_id: "audio_in" },
        }, rackGraphId("edge.audio"));
      }
    } else {
      let audioInput = graph.nodes.find(
        (node) => node.kind.kind === "audio_input" && node.kind.bus_id === "main",
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
        graph = { ...graph, nodes: [...graph.nodes, audioInput] };
      }
      // The input's one cable is left where it is if it is already patched;
      // the effect is then fed by whatever is drawn to it.
      if (!graph.edges.some((edge) =>
        edge.signal === "audio" && edge.source.node_id === audioInput!.id)) {
        graph = connectRackGraph(graph, {
          signal: "audio",
          source: { node_id: audioInput.id, port_id: "out" },
          target: { node_id: nodeId, port_id: "audio_in" },
        }, rackGraphId("edge.audio-in"));
      }
    }
  } else {
    graph = {
      ...graph,
      edges: [
        ...graph.edges,
        {
          id: rackGraphId("edge.midi"),
          signal: "midi",
          source: { node_id: midiInput.id, port_id: "out" },
          target: { node_id: nodeId, port_id: "midi_in" },
          midi_transform: midiTransformFromSlot(slot),
        },
      ],
    };
  }

  const destination = role === "effect"
    ? audioOutput.id
    : sharedInstrumentDestination(graph, nodeId) ?? audioOutput.id;
  graph = connectRackGraph(graph, {
    signal: "audio",
    source: { node_id: nodeId, port_id: "audio_out" },
    target: {
      node_id: destination,
      port_id: nodeKind(graph, destination) === "plugin" ? "audio_in" : "in",
    },
  }, rackGraphId("edge.audio"));

  return { ...current, slots: [...current.slots, slot], graph };
}

const NODE_SPACING_X = 300;
const NODE_SPACING_Y = 160;

/**
 * Where a node added without a place of its own goes, so it lands where it
 * is wired rather than on top of another. An effect goes just after what it
 * is inserted behind, level with it, and the output steps right to make room
 * when it has to; a pedalboard's first effect goes after the audio input. An
 * instrument goes under the instruments already there.
 */
function placeNewNode(
  graph: RackGraph,
  role: RackPluginRole,
  midiInput: RackGraphNode,
  audioOutput: RackGraphNode,
): { position: RackGraphPosition; outputPosition?: RackGraphPosition } {
  const at = (id: string) => graph.nodes.find((node) => node.id === id)!.position;
  if (role === "effect") {
    const feeders = graph.edges
      .filter((edge) =>
        edge.signal === "audio"
        && edge.target.node_id === audioOutput.id
        && nodeKind(graph, edge.source.node_id) === "plugin")
      .map((edge) => at(edge.source.node_id));
    const input = graph.nodes.find(
      (node) => node.kind.kind === "audio_input" && node.kind.bus_id === "main",
    );
    const behind = feeders.length > 0 ? feeders : input ? [input.position] : [];
    if (behind.length === 0) {
      return { position: normalizeRackGraphPosition({ x: 360, y: audioOutput.position.y }) };
    }
    const position = normalizeRackGraphPosition({
      x: Math.max(...behind.map((one) => one.x)) + NODE_SPACING_X,
      y: behind.reduce((sum, one) => sum + one.y, 0) / behind.length,
    });
    return audioOutput.position.x < position.x + NODE_SPACING_X
      ? {
        position,
        outputPosition: { x: position.x + NODE_SPACING_X, y: audioOutput.position.y },
      }
      : { position };
  }
  const instruments = graph.nodes.filter((node) =>
    node.kind.kind === "plugin"
    && !graph.edges.some((edge) => edge.signal === "audio" && edge.target.node_id === node.id));
  if (instruments.length === 0) {
    return {
      position: normalizeRackGraphPosition({
        x: midiInput.position.x + NODE_SPACING_X,
        y: midiInput.position.y,
      }),
    };
  }
  return {
    position: normalizeRackGraphPosition({
      x: Math.min(...instruments.map((node) => node.position.x)),
      y: Math.max(...instruments.map((node) => node.position.y)) + NODE_SPACING_Y,
    }),
  };
}

/**
 * Where the Rack's instruments send their audio, when they all send it to the
 * same effect: the head of the chain a new instrument should join. Null when
 * there are none, when they go straight to an output, or when they disagree.
 * An instrument here is a plugin node that takes no audio in.
 */
function sharedInstrumentDestination(graph: RackGraph, excluding: string): string | null {
  const instruments = graph.nodes.filter((node) =>
    node.kind.kind === "plugin"
    && node.id !== excluding
    && !graph.edges.some((edge) => edge.signal === "audio" && edge.target.node_id === node.id));
  const destinations = new Set(
    instruments.flatMap((node) => graph.edges
      .filter((edge) => edge.signal === "audio" && edge.source.node_id === node.id)
      .map((edge) => edge.target.node_id)),
  );
  if (instruments.length === 0 || destinations.size !== 1) return null;
  const [destination] = destinations;
  return nodeKind(graph, destination) === "plugin" ? destination : null;
}

/**
 * Removes a Slot and its node, and closes the gap it leaves: whatever fed the
 * node's audio input now feeds wherever the node sent its audio, so taking an
 * effect out of a chain leaves the instrument behind it still heard, not cut
 * off. A cable the rules would refuse -- the bare audio input straight to an
 * output -- is not drawn.
 */
export function removeSlotFromRack(rack: RackDefinition, slotId: string): RackDefinition {
  const current = materializeRackGraph(rack);
  const graph = current.graph!;
  const nodeIds = new Set(
    graph.nodes
      .filter((node) => node.kind.kind === "plugin" && node.kind.slot_id === slotId)
      .map((node) => node.id),
  );
  let next: RackGraph = {
    ...graph,
    nodes: graph.nodes.filter((node) => !nodeIds.has(node.id)),
    edges: graph.edges.filter(
      (edge) => !nodeIds.has(edge.source.node_id) && !nodeIds.has(edge.target.node_id),
    ),
  };
  for (const nodeId of nodeIds) {
    const sources = graph.edges.filter(
      (edge) => edge.signal === "audio" && edge.target.node_id === nodeId,
    );
    const destinations = graph.edges.filter(
      (edge) => edge.signal === "audio" && edge.source.node_id === nodeId,
    );
    if (destinations.length !== 1) continue;
    for (const source of sources) {
      if (nodeIds.has(source.source.node_id)) continue;
      next = connectRackGraph(next, {
        signal: "audio",
        source: source.source,
        target: destinations[0].target,
      }, rackGraphId("edge.audio"));
    }
  }
  return {
    ...current,
    slots: current.slots.filter((slot) => slot.id !== slotId),
    graph: next,
  };
}
