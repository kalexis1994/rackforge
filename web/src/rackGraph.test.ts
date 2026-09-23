import { describe, expect, it } from "vitest";
import {
  addSlotToRack,
  connectRackGraph,
  graphFromSlots,
  rackConnectionProblem,
  rackGraphProblems,
  removeSlotFromRack,
} from "./rackGraph";
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

function mainOutput(rack: RackDefinition) {
  return rack.graph!.nodes.find(
    (one) => one.kind.kind === "audio_output" && one.kind.bus_id === "main",
  )!;
}

/** Where a node's audio goes, hop by hop: Slot ids, then "output". Fails if a
 *  node sends its audio to more than one place. */
function chainOf(rack: RackDefinition, nodeId: string): string[] {
  const hops: string[] = [];
  let current = nodeId;
  for (;;) {
    const out = rack.graph!.edges.filter(
      (edge) => edge.signal === "audio" && edge.source.node_id === current,
    );
    if (out.length === 0) return hops;
    expect(out).toHaveLength(1);
    const next = rack.graph!.nodes.find((one) => one.id === out[0].target.node_id)!;
    if (next.kind.kind === "audio_output") return [...hops, "output"];
    if (next.kind.kind === "plugin") hops.push(next.kind.slot_id);
    current = next.id;
  }
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

  it("chains a second effect after the first on a pedalboard", () => {
    // Two effects and two hardware inputs would be two different guitars; two
    // effects both fed by the one input would be the guitar heard twice.
    const first = addSlotToRack(emptyRack(), slot("drive", "rf-rig"), undefined, "effect");
    const second = addSlotToRack(first, slot("verb", "rf-verb"), undefined, "effect");

    const inputs = second.graph!.nodes.filter((one) => one.kind.kind === "audio_input");
    expect(inputs).toHaveLength(1);
    expect(chainOf(second, inputs[0].id)).toEqual(["drive", "verb", "output"]);
  });

  it("inserts an effect between an instrument and the output", () => {
    const rack = addSlotToRack(
      addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano")),
      slot("comp", "rf-comp"),
      undefined,
      "effect",
    );
    expect(chainOf(rack, nodeFor(rack, "piano").id)).toEqual(["comp", "output"]);
    // No hardware input: the effect is fed by the instrument.
    expect(rack.graph!.nodes.some((one) => one.kind.kind === "audio_input")).toBe(false);
  });

  it("inserts an effect after the instruments, all of them", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("strings", "org.rackforge.strings"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");

    for (const instrument of ["piano", "strings"]) {
      expect(chainOf(rack, nodeFor(rack, instrument).id)).toEqual(["verb", "output"]);
    }
  });

  it("adds a later effect at the end of the chain", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("comp", "rf-comp"), undefined, "effect");
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");

    expect(chainOf(rack, nodeFor(rack, "piano").id)).toEqual(["comp", "verb", "output"]);
  });

  it("joins a new instrument to the effects the others go through", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    rack = addSlotToRack(rack, slot("strings", "org.rackforge.strings"));

    expect(chainOf(rack, nodeFor(rack, "strings").id)).toEqual(["verb", "output"]);
  });

  it("sends a new instrument to the output when the others disagree", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    rack = addSlotToRack(rack, slot("pad", "org.rackforge.pad"));
    // Re-patch the pad straight to the output: now the instruments disagree.
    const output = mainOutput(rack);
    rack = {
      ...rack,
      graph: connectRackGraph(rack.graph!, {
        signal: "audio",
        source: { node_id: nodeFor(rack, "pad").id, port_id: "audio_out" },
        target: { node_id: output.id, port_id: "in" },
      }),
    };
    rack = addSlotToRack(rack, slot("organ", "org.rackforge.organ"));

    expect(chainOf(rack, nodeFor(rack, "organ").id)).toEqual(["output"]);
  });

  it("inserts an effect right after the port a cable was dropped from", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    rack = addSlotToRack(rack, slot("comp", "rf-comp"), { x: 400, y: 300 }, "effect", {
      insertAfter: { node_id: nodeFor(rack, "piano").id, port_id: "audio_out" },
    });

    expect(chainOf(rack, nodeFor(rack, "piano").id)).toEqual(["comp", "verb", "output"]);
    expect(nodeFor(rack, "comp").position).toEqual({ x: 400, y: 300 });
    expect(rackGraphProblems(rack.graph!)).toEqual([]);
  });

  it("sends an effect dropped after an unpatched output to the main output", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    const piano = nodeFor(rack, "piano").id;
    rack = { ...rack, graph: { ...rack.graph!, edges: rack.graph!.edges.filter(
      (edge) => !(edge.signal === "audio" && edge.source.node_id === piano),
    ) } };
    rack = addSlotToRack(rack, slot("comp", "rf-comp"), undefined, "effect", {
      insertAfter: { node_id: piano, port_id: "audio_out" },
    });
    expect(chainOf(rack, piano)).toEqual(["comp", "output"]);
  });

  it("gives every audio output one destination, whatever is added", () => {
    let rack = emptyRack();
    for (const [id, role] of [
      ["piano", "instrument"], ["comp", "effect"], ["strings", "instrument"],
      ["verb", "effect"], ["pad", "instrument"],
    ] as const) {
      rack = addSlotToRack(rack, slot(id, `org.${id}`), undefined, role);
      expect(rackGraphProblems(rack.graph!)).toEqual([]);
    }
  });

  it("creates missing ends instead of rebuilding the graph", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    const piano = nodeFor(rack, "piano");
    // An output for another bus, which the Rack does not have yet.
    const withBus = addSlotToRack(rack, { ...slot("drums", "org.drums"), audio_output_bus: "aux" });

    expect(nodeFor(withBus, "piano")).toEqual(piano);
    expect(chainOf(withBus, piano.id)).toEqual(["verb", "output"]);
    expect(
      withBus.graph!.nodes.filter((one) => one.kind.kind === "audio_output"),
    ).toHaveLength(2);
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

describe("removing a Slot from a Rack", () => {
  it("closes the gap an effect leaves in a chain", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("comp", "rf-comp"), undefined, "effect");
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    rack = removeSlotFromRack(rack, "comp");

    expect(chainOf(rack, nodeFor(rack, "piano").id)).toEqual(["verb", "output"]);
    expect(rackGraphProblems(rack.graph!)).toEqual([]);
  });

  it("reconnects every instrument an effect was mixing", () => {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("strings", "org.rackforge.strings"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    rack = removeSlotFromRack(rack, "verb");

    for (const instrument of ["piano", "strings"]) {
      expect(chainOf(rack, nodeFor(rack, instrument).id)).toEqual(["output"]);
    }
  });

  it("does not wire the bare audio input to the output", () => {
    let rack = addSlotToRack(emptyRack(), slot("drive", "rf-rig"), undefined, "effect");
    rack = removeSlotFromRack(rack, "drive");
    const input = rack.graph!.nodes.find((one) => one.kind.kind === "audio_input")!;

    expect(edgesOutOf(rack, input.id)).toEqual([]);
  });
});

describe("the graph's connection rules", () => {
  function pianoRack() {
    let rack = addSlotToRack(emptyRack(), slot("piano", "org.rackforge.piano"));
    rack = addSlotToRack(rack, slot("verb", "rf-verb"), undefined, "effect");
    return rack;
  }

  it("moves an audio cable instead of adding a second", () => {
    const rack = pianoRack();
    const piano = nodeFor(rack, "piano");
    const graph = connectRackGraph(rack.graph!, {
      signal: "audio",
      source: { node_id: piano.id, port_id: "audio_out" },
      target: { node_id: mainOutput(rack).id, port_id: "in" },
    });

    const outgoing = graph.edges.filter(
      (edge) => edge.signal === "audio" && edge.source.node_id === piano.id,
    );
    expect(outgoing).toHaveLength(1);
    expect(outgoing[0].target.node_id).toBe(mainOutput(rack).id);
  });

  it("lets several sources mix into one input", () => {
    let rack = pianoRack();
    rack = addSlotToRack(rack, slot("strings", "org.rackforge.strings"));
    const verb = nodeFor(rack, "verb");
    expect(edgesInto(rack, verb.id).filter((edge) => edge.signal === "audio")).toHaveLength(2);
  });

  it("refuses a loop", () => {
    const rack = pianoRack();
    expect(rackConnectionProblem(rack.graph!, {
      signal: "audio",
      source: { node_id: nodeFor(rack, "verb").id, port_id: "audio_out" },
      target: { node_id: nodeFor(rack, "piano").id, port_id: "audio_in" },
    })).toMatch(/loop/);
  });

  it("refuses a node feeding itself", () => {
    const rack = pianoRack();
    const verb = nodeFor(rack, "verb").id;
    expect(rackConnectionProblem(rack.graph!, {
      signal: "audio",
      source: { node_id: verb, port_id: "audio_out" },
      target: { node_id: verb, port_id: "audio_in" },
    })).not.toBeNull();
  });

  it("refuses the audio input straight to an output", () => {
    const rack = addSlotToRack(emptyRack(), slot("drive", "rf-rig"), undefined, "effect");
    const input = rack.graph!.nodes.find((one) => one.kind.kind === "audio_input")!;
    expect(rackConnectionProblem(rack.graph!, {
      signal: "audio",
      source: { node_id: input.id, port_id: "out" },
      target: { node_id: mainOutput(rack).id, port_id: "in" },
    })).not.toBeNull();
  });

  it("keeps a child Rack on its own cable to the main output", () => {
    const rack = pianoRack();
    const graph = {
      ...rack.graph!,
      nodes: [
        ...rack.graph!.nodes,
        { id: "rack.layer", kind: { kind: "rack" as const, rack_id: "rack.child" }, position: { x: 0, y: 0 } },
      ],
    };
    expect(rackConnectionProblem(graph, {
      signal: "audio",
      source: { node_id: "rack.layer", port_id: "audio_out" },
      target: { node_id: nodeFor(rack, "verb").id, port_id: "audio_in" },
    })).not.toBeNull();
    expect(rackConnectionProblem(graph, {
      signal: "audio",
      source: { node_id: nodeFor(rack, "piano").id, port_id: "audio_out" },
      target: { node_id: "rack.layer", port_id: "audio_in" },
    })).not.toBeNull();
    expect(rackConnectionProblem(graph, {
      signal: "audio",
      source: { node_id: "rack.layer", port_id: "audio_out" },
      target: { node_id: mainOutput(rack).id, port_id: "in" },
    })).toBeNull();
  });

  it("leaves the graph alone when a cable is refused", () => {
    const rack = pianoRack();
    const verb = nodeFor(rack, "verb").id;
    const graph = connectRackGraph(rack.graph!, {
      signal: "audio",
      source: { node_id: verb, port_id: "audio_out" },
      target: { node_id: nodeFor(rack, "piano").id, port_id: "audio_in" },
    });
    expect(graph).toBe(rack.graph);
  });

  it("names the main output when nothing reaches it", () => {
    const empty = emptyRack();
    expect(rackGraphProblems(empty.graph!).map((problem) => problem.nodeId)).toEqual([
      mainOutput(empty).id,
    ]);
    const playing = addSlotToRack(empty, slot("piano", "org.rackforge.piano"));
    expect(rackGraphProblems(playing.graph!)).toEqual([]);
  });

  it("names a node whose audio goes two ways, or nowhere", () => {
    const rack = pianoRack();
    const piano = nodeFor(rack, "piano").id;
    const doubled = {
      ...rack.graph!,
      edges: [
        ...rack.graph!.edges,
        {
          id: "edge.extra",
          signal: "audio" as const,
          source: { node_id: piano, port_id: "audio_out" },
          target: { node_id: mainOutput(rack).id, port_id: "in" },
        },
      ],
    };
    expect(rackGraphProblems(doubled).map((problem) => problem.nodeId)).toContain(piano);

    const silent = {
      ...rack.graph!,
      edges: rack.graph!.edges.filter(
        (edge) => !(edge.signal === "audio" && edge.source.node_id === nodeFor(rack, "verb").id),
      ),
    };
    expect(rackGraphProblems(silent).map((problem) => problem.nodeId)).toContain(
      nodeFor(rack, "verb").id,
    );
  });
});
