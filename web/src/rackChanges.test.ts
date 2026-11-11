import { describe, expect, it } from "vitest";
import { addSlotToRack, connectRackGraph, graphFromSlots, insertNodeIntoCable, removeSlotFromRack } from "./rackGraph";
import { describeRackChange } from "./rackChanges";
import type { RackAudioInputRoute, RackDefinition, RackSlot } from "./types";

function slot(id: string, name = id): RackSlot {
  return {
    id,
    name,
    plugin_id: `org.${id}`,
    enabled: true,
    midi_note_low: 0,
    midi_note_high: 127,
    midi_transpose: 0,
    midi_output: { kind: "none" },
    audio_output_bus: "main",
    level_per_mille: 1000,
    pan_per_mille: 0,
  };
}

const empty: RackDefinition = {
  schema_version: 1,
  id: "rack.test",
  name: "Test",
  enabled: true,
  slots: [],
  graph: graphFromSlots([]),
};

describe("naming a Rack edit", () => {
  const piano = addSlotToRack(empty, slot("piano", "Piano"));
  const withComp = addSlotToRack(piano, slot("comp", "RF-Comp"), undefined, "effect");

  it("names a plugin added, and one inserted into the chain", () => {
    expect(describeRackChange(empty, piano)).toBe("Added Piano");
    expect(describeRackChange(piano, withComp)).toBe("Added RF-Comp · inserted into the chain");
  });

  it("names a plugin removed, and the chain closed behind it", () => {
    const removed = removeSlotFromRack(withComp, "comp");
    expect(describeRackChange(withComp, removed)).toBe("Removed RF-Comp · chain reconnected");
  });

  it("names an audio input cable given its own inputs and trim", () => {
    const pedal = addSlotToRack(empty, slot("drive", "RF-Drive"), undefined, "effect");
    const input = pedal.graph!.nodes.find((node) => node.kind.kind === "audio_input")!;
    const route = (audio_input_route: RackAudioInputRoute) => ({
      ...pedal,
      graph: {
        ...pedal.graph!,
        edges: pedal.graph!.edges.map((edge) =>
          edge.source.node_id === input.id ? { ...edge, audio_input_route } : edge),
      },
    });
    expect(describeRackChange(pedal, route({ channels: [2] }))).toBe("Audio input: In 2 to RF-Drive");
    expect(describeRackChange(pedal, route({ channels: [1, 2], gain_db: -6 })))
      .toBe("Audio input: In 1–2, -6 dB to RF-Drive");
  });

  it("names a cable moved from an output", () => {
    const pianoNode = withComp.graph!.nodes.find(
      (node) => node.kind.kind === "plugin" && node.kind.slot_id === "piano",
    )!;
    const output = withComp.graph!.nodes.find((node) => node.kind.kind === "audio_output")!;
    const repatched = {
      ...withComp,
      graph: connectRackGraph(withComp.graph!, {
        signal: "audio",
        source: { node_id: pianoNode.id, port_id: "audio_out" },
        target: { node_id: output.id, port_id: "in" },
      }),
    };
    expect(describeRackChange(withComp, repatched)).toBe("Re-patched Piano → Audio Output");
  });

  it("names a node dropped into a cable", () => {
    const freeEq = addSlotToRack(withComp, slot("eq", "RF-EQ"), undefined, "effect");
    const eq = freeEq.graph!.nodes.find(
      (node) => node.kind.kind === "plugin" && node.kind.slot_id === "eq",
    )!.id;
    const unplugged = removeSlotFromRack(freeEq, "eq");
    const loose = {
      ...unplugged,
      slots: freeEq.slots,
      graph: {
        ...unplugged.graph!,
        nodes: [...unplugged.graph!.nodes, freeEq.graph!.nodes.find((node) => node.id === eq)!],
      },
    };
    const cable = loose.graph.edges.find((edge) =>
      edge.signal === "audio" && edge.target.node_id !== eq
      && loose.graph.nodes.find((node) => node.id === edge.source.node_id)?.kind.kind === "plugin"
      && (loose.graph.nodes.find((node) => node.id === edge.source.node_id)!.kind as { slot_id: string }).slot_id === "piano")!;
    const inserted = { ...loose, graph: insertNodeIntoCable(loose.graph, eq, cable.id) };
    expect(describeRackChange(loose, inserted)).toBe("Inserted RF-EQ between Piano and RF-Comp");
  });

  it("names a move and a rename", () => {
    const moved = {
      ...piano,
      graph: {
        ...piano.graph!,
        nodes: piano.graph!.nodes.map((node) =>
          node.kind.kind === "plugin" ? { ...node, position: { x: 999, y: 999 } } : node),
      },
    };
    expect(describeRackChange(piano, moved)).toBe("Moved Piano");
    expect(describeRackChange(piano, { ...piano, name: "Stage" })).toBe("Renamed Rack");
  });
});
