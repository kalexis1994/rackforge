import { describe, expect, it } from "vitest";
import { addSlotToRack, graphFromSlots } from "./rackGraph";
import type { RackDefinition, RackSlot } from "./types";

function slot(id: string, pluginId: string): RackSlot {
  return {
    id,
    name: id,
    plugin_id: pluginId,
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

function emptyRack(): RackDefinition {
  return {
    schema_version: 1,
    id: "rack.test",
    name: "Test Rack",
    enabled: true,
    slots: [],
    graph: graphFromSlots([]),
  };
}

function nodeFor(rack: RackDefinition, slotId: string) {
  return rack.graph!.nodes.find(
    (node) => node.kind.kind === "plugin" && node.kind.slot_id === slotId,
  )!;
}

function edgesInto(rack: RackDefinition, nodeId: string) {
  return rack.graph!.edges.filter((edge) => edge.target.node_id === nodeId);
}

function edgesOutOf(rack: RackDefinition, nodeId: string) {
  return rack.graph!.edges.filter((edge) => edge.source.node_id === nodeId);
}

describe("adding a Slot to a Rack", () => {
  it("drives an instrument from the MIDI input", () => {
    const rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    const node = nodeFor(rack, "piano");
    const midiInput = rack.graph!.nodes.find((one) => one.kind.kind === "midi_input")!;

    const incoming = edgesInto(rack, node.id);
    expect(incoming).toHaveLength(1);
    expect(incoming[0].signal).toBe("midi");
    expect(incoming[0].source.node_id).toBe(midiInput.id);
    expect(incoming[0].target.port_id).toBe("midi_in");
    expect(incoming[0].midi_transform).toBeDefined();
  });

  it("feeds an effect from the audio input instead", () => {
    const rack = addSlotToRack(
      emptyRack(),
      slot("pedalboard", "org.rackforge.rf-rig"),
      undefined,
      "effect",
    );
    const node = nodeFor(rack, "pedalboard");

    const incoming = edgesInto(rack, node.id);
    expect(incoming).toHaveLength(1);
    expect(incoming[0].signal).toBe("audio");
    expect(incoming[0].target.port_id).toBe("audio_in");
    expect(incoming[0].midi_transform).toBeUndefined();

    const source = rack.graph!.nodes.find((one) => one.id === incoming[0].source.node_id)!;
    expect(source.kind).toEqual({ kind: "audio_input", bus_id: "main" });
  });

  it("creates the audio input the Rack was missing", () => {
    const before = emptyRack();
    expect(before.graph!.nodes.some((one) => one.kind.kind === "audio_input")).toBe(false);

    const rack = addSlotToRack(before, slot("pedalboard", "rf-rig"), undefined, "effect");
    expect(
      rack.graph!.nodes.filter((one) => one.kind.kind === "audio_input"),
    ).toHaveLength(1);
  });

  it("reuses the audio input a second effect already has", () => {
    // Two effects and two hardware inputs would be two different guitars.
    const first = addSlotToRack(emptyRack(), slot("drive", "rf-rig"), undefined, "effect");
    const second = addSlotToRack(first, slot("verb", "rf-verb"), undefined, "effect");

    const inputs = second.graph!.nodes.filter((one) => one.kind.kind === "audio_input");
    expect(inputs).toHaveLength(1);
    expect(edgesOutOf(second, inputs[0].id)).toHaveLength(2);
  });

  it("sends both kinds of Slot to the output bus", () => {
    const rack = addSlotToRack(
      addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano")),
      slot("pedalboard", "rf-rig"),
      undefined,
      "effect",
    );
    const output = rack.graph!.nodes.find(
      (one) => one.kind.kind === "audio_output" && one.kind.bus_id === "main",
    )!;

    for (const slotId of ["piano", "pedalboard"]) {
      const outgoing = edgesOutOf(rack, nodeFor(rack, slotId).id);
      expect(outgoing).toHaveLength(1);
      expect(outgoing[0].signal).toBe("audio");
      expect(outgoing[0].source.port_id).toBe("audio_out");
      expect(outgoing[0].target.node_id).toBe(output.id);
    }
  });

  it("keeps both Slots in the Rack", () => {
    const rack = addSlotToRack(
      addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano")),
      slot("pedalboard", "rf-rig"),
      undefined,
      "effect",
    );

    expect(rack.slots.map((one) => one.id)).toEqual(["piano", "pedalboard"]);
  });
});
