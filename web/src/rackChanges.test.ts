import { describe, expect, it } from "vitest";
import { addSlotToRack, connectRackGraph, graphFromSlots, removeSlotFromRack } from "./rackGraph";
import { describeRackChange } from "./rackChanges";
import type { RackDefinition, RackSlot } from "./types";

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
