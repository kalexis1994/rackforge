import { describe, expect, it } from "vitest";
import {
  buildControllerDevices,
  type ControllerInput,
  defaultMode,
  describeMode,
  emptyControllerMap,
  groupInputs,
  inputForActivity,
  inputFromActivity,
  inputMessageLabel,
  isUserController,
  kindsForInput,
  mappedInputFor,
  mappingsForInput,
  modeProblem,
  modesFor,
  newMappingId,
  parameterSections,
  roleLabel,
  standardMeaning,
  suggestedControllerName,
  suggestedMode,
  unknownSourceDevices,
  userControllerProblem,
  withLearntInput,
  withMapping,
  withoutMapping,
} from "./controllerMapping";
import type { ControlMapping, PluginParameterDescriptor, PluginParameterSnapshot } from "./types";

function parameter(
  id: string,
  kind: PluginParameterDescriptor["kind"],
  extra: Partial<PluginParameterDescriptor> = {},
): PluginParameterDescriptor {
  return {
    index: 0,
    id,
    name: id,
    page: "main",
    order: 0,
    kind,
    flags: { automatable: true, modulatable: false, read_only: false, advanced: false },
    suggested_control: "knob",
    ...extra,
  };
}

const leslie = parameter("leslie.speed", {
  type: "enum",
  default: 0,
  choices: [
    { value: 0, name: "Stop" },
    { value: 1, name: "Chorale" },
    { value: 2, name: "Tremolo" },
  ],
}, { name: "Leslie speed" });
const drive = parameter("drive", { type: "float", minimum: 0, maximum: 10, default: 0, step: 0.1, unit: "dB" });
const sustain = parameter("sustain", { type: "boolean", default: false });
const reset = parameter("reset", { type: "trigger" });
const meter = parameter("level", { type: "meter", minimum: 0, maximum: 1 });

const knob: ControllerInput = { id: "knob-1", name: "Knob 1", kind: "knob", group: "Knobs", midi: { channel: 0, cc: 74 } };
const button: ControllerInput = { id: "button-1", name: "Button 1", kind: "button", group: "Buttons", midi: { channel: 0, cc: 20 } };
const pad: ControllerInput = { id: "pad-1", name: "Pad 1", kind: "pad", midi: { channel: 9, note: 36 } };
const wheel: ControllerInput = { id: "bend", name: "Pitch", kind: "wheel", midi: { channel: 0, pitch_bend: true } };

describe("which control a message comes from", () => {
  const inputs = [knob, button, pad, wheel];

  it("matches control changes, notes and bends on their channel", () => {
    expect(inputForActivity({ status: 0xb0, data1: 74 }, inputs)).toBe(knob);
    expect(inputForActivity({ status: 0x99, data1: 36 }, inputs)).toBe(pad);
    expect(inputForActivity({ status: 0x89, data1: 36 }, inputs)).toBe(pad);
    expect(inputForActivity({ status: 0xe0, data1: 0 }, inputs)).toBe(wheel);
  });

  it("ignores the same number on another channel or another kind", () => {
    expect(inputForActivity({ status: 0xb1, data1: 74 }, inputs)).toBeUndefined();
    expect(inputForActivity({ status: 0x90, data1: 74 }, inputs)).toBeUndefined();
    expect(inputForActivity({ status: 0xd0, data1: 74 }, inputs)).toBeUndefined();
  });

  it("makes an input of a message RackForge does not know, keyed to it", () => {
    expect(inputFromActivity({ status: 0xb2, data1: 7, data2: 100 })).toMatchObject({
      id: "cc-3-7",
      kind: "knob",
      midi: { channel: 2, cc: 7 },
    });
    expect(inputFromActivity({ status: 0x99, data1: 36, data2: 90 })).toMatchObject({ id: "note-10-36", kind: "pad" });
    expect(inputFromActivity({ status: 0x99, data1: 36, data2: 0 })).toBeNull();
    expect(inputFromActivity({ status: 0xe0, data1: 0, data2: 64 })).toMatchObject({ kind: "wheel" });
    expect(inputFromActivity({ status: 0xc0, data1: 3, data2: 0 })).toBeNull();
  });

  it("says what a control sends as the hardware would", () => {
    expect(inputMessageLabel(knob)).toBe("CC 74 · ch 1");
    expect(inputMessageLabel(pad)).toBe("Note 36 · ch 10");
    expect(inputMessageLabel(wheel)).toBe("Pitch bend · ch 1");
  });

  it("records an input with a copy of its message, channels counted from one", () => {
    expect(mappedInputFor(pad)).toEqual({
      id: "pad-1",
      name: "Pad 1",
      channel: { mode: "channel", channel: 10 },
      message: { type: "note", note: 36 },
    });
  });
});

describe("what an input can do to a parameter", () => {
  it("offers buttons presses and knobs positions", () => {
    expect(modesFor(button, leslie)).toEqual(["cycle", "toggle", "set", "hold", "step"]);
    expect(modesFor(pad, reset)).toEqual(["trigger"]);
    expect(modesFor(knob, drive)).toEqual(["direct", "range"]);
    expect(modesFor(knob, leslie)).toEqual(["zones", "direct"]);
    expect(modesFor(knob, meter)).toEqual([]);
  });

  it("suggests a cycle through the Leslie for a button", () => {
    expect(suggestedMode(button, leslie)).toEqual({ kind: "cycle", values: [0, 1, 2] });
    expect(suggestedMode(button, sustain)).toEqual({ kind: "toggle", first: 0, second: 1 });
    expect(suggestedMode(knob, drive)).toEqual({ kind: "direct" });
  });

  it("fills a mode in with values the parameter accepts", () => {
    expect(defaultMode("toggle", leslie)).toEqual({ kind: "toggle", first: 0, second: 1 });
    expect(defaultMode("range", drive)).toEqual({ kind: "range", min: 0, max: 10 });
    expect(defaultMode("hold", leslie)).toEqual({ kind: "hold", pressed: 1, released: 0 });
    for (const kind of modesFor(button, leslie)) {
      expect(modeProblem(button, leslie, defaultMode(kind, leslie))).toBeNull();
    }
  });

  it("refuses what the host would refuse", () => {
    expect(modeProblem(button, leslie, { kind: "set", value: 7 })).toMatch(/does not accept 7/);
    expect(modeProblem(button, leslie, { kind: "toggle", first: 1, second: 1 })).toMatch(/two different/);
    expect(modeProblem(button, leslie, { kind: "cycle", values: [1] })).toMatch(/at least two/);
    expect(modeProblem(knob, drive, { kind: "range", min: 3, max: 3 })).toMatch(/must differ/);
    expect(modeProblem(knob, drive, { kind: "range", min: 0, max: 12 })).toMatch(/does not accept 12/);
    expect(modeProblem(knob, leslie, { kind: "range", min: 0, max: 2 })).toMatch(/does not fit/);
    expect(modeProblem(button, meter, { kind: "trigger" })).toMatch(/cannot be driven/);
  });

  it("says a mode in the parameter's own words", () => {
    expect(describeMode({ kind: "toggle", first: 1, second: 2 }, leslie)).toBe("Toggle · Chorale ↔ Tremolo");
    expect(describeMode({ kind: "range", min: 2, max: 6.5 }, drive)).toBe("Range · 2 dB – 6.5 dB");
    expect(describeMode(undefined)).toBe("Direct");
    expect(describeMode({ kind: "step", direction: "down", wrap: true })).toBe("Step down · wraps");
  });
});

describe("a map as it is edited", () => {
  const organ = { plugin_id: "org.rackforge.organ", plugin_name: "RF-Organ" };
  const synth = { plugin_id: "org.rackforge.rf-106", plugin_name: "RF-106" };

  function mapping(id: string, input: ControllerInput, parameterId: string): ControlMapping {
    return { id, input: mappedInputFor(input)!, parameter_id: parameterId, mode: { kind: "direct" } };
  }

  it("gives an input one thing to do in each plugin", () => {
    let map = emptyControllerMap("user.oxygen-49", "Oxygen 49");
    map = withMapping(map, organ, mapping("a", button, "leslie.speed"));
    map = withMapping(map, synth, mapping("b", button, "chorus"));
    // The same button again in the organ replaces what it did there.
    map = withMapping(map, organ, mapping("c", button, "percussion"));
    expect(map.plugins).toHaveLength(2);
    expect(map.plugins.find((plugin) => plugin.plugin_id === organ.plugin_id)?.mappings.map((m) => m.id)).toEqual(["c"]);
    expect(mappingsForInput(map, button).map((entry) => entry.plugin_id).sort()).toEqual([
      organ.plugin_id,
      synth.plugin_id,
    ]);
  });

  it("knows a control under another name by its message", () => {
    const learnt: ControllerInput = { ...button, id: "cc-1-20", name: "CC 20" };
    let map = withMapping(emptyControllerMap("x", "X"), organ, mapping("a", learnt, "leslie.speed"));
    expect(mappingsForInput(map, button)).toHaveLength(1);
    map = withMapping(map, organ, mapping("b", button, "percussion"));
    expect(map.plugins[0].mappings.map((m) => m.id)).toEqual(["b"]);
  });

  it("drops a plugin left with nothing", () => {
    let map = withMapping(emptyControllerMap("x", "X"), organ, mapping("a", button, "leslie.speed"));
    map = withoutMapping(map, organ.plugin_id, "a");
    expect(map.plugins).toEqual([]);
  });

  it("leaves the map it was given as it was", () => {
    const before = withMapping(emptyControllerMap("x", "X"), organ, mapping("a", button, "leslie.speed"));
    const snapshot = JSON.stringify(before);
    withMapping(before, organ, mapping("b", knob, "drive"));
    withoutMapping(before, organ.plugin_id, "a");
    expect(JSON.stringify(before)).toBe(snapshot);
  });

  it("makes ids the host accepts, different every time", () => {
    const ids = new Set(Array.from({ length: 200 }, () => newMappingId()));
    expect(ids.size).toBe(200);
    for (const id of ids) expect(id).toMatch(/^[a-z0-9][a-z0-9._-]*$/);
  });
});

describe("which controllers the editor shows", () => {
  const keylab = {
    id: "org.rackforge.keylab",
    name: "KeyLab",
    version: "1",
    enabled: true,
    trust: "official",
    runtime: "ProcessV1",
    inputs: [knob],
  };
  const oxygen = { ...keylab, id: "user.oxygen", name: "Oxygen", inputs: [button] };
  const disabled = { ...keylab, id: "user.off", name: "Off", enabled: false };

  it("puts attached controllers first, and keeps a map whose package is gone", () => {
    const orphanMap = withMapping(emptyControllerMap("user.gone", "Old keyboard"), {
      plugin_id: "org.rackforge.organ",
      plugin_name: "RF-Organ",
    }, { id: "a", input: mappedInputFor(pad)!, parameter_id: "leslie.speed", mode: { kind: "trigger" } });
    const devices = buildControllerDevices(
      [keylab, oxygen, disabled],
      [{ controller_id: "user.oxygen", source: { id: "alsa.oxygen", name: "Oxygen 49 MIDI" }, connected: true }],
      [orphanMap],
    );
    expect(devices.map((device) => device.id)).toEqual(["user.oxygen", "org.rackforge.keylab", "user.gone"]);
    expect(devices[0]).toMatchObject({ connected: true, source: { id: "alsa.oxygen" } });
    const gone = devices[2];
    expect(gone).toMatchObject({ name: "Old keyboard", orphaned: true });
    expect(gone.inputs).toEqual([{ id: "pad-1", name: "Pad 1", kind: "pad", midi: { channel: 9, note: 36 } }]);
  });
});

describe("a controller RackForge does not know", () => {
  it("is an enabled input no package claims", () => {
    const devices = unknownSourceDevices(
      [
        { source: { id: "alsa.keylab", name: "KeyLab" }, connected: true },
        { source: { id: "alsa.oxygen", name: "Oxygen 49" }, connected: true },
      ],
      new Set(["alsa.keylab"]),
      new Map([["alsa.oxygen", [knob]]]),
    );
    expect(devices).toHaveLength(1);
    expect(devices[0]).toMatchObject({ id: "midi:alsa.oxygen", name: "Oxygen 49", unknown: true, inputs: [knob] });
  });

  it("takes its name from the input it was heard on", () => {
    expect(suggestedControllerName("Oxygen 49:Oxygen 49 MIDI 1 24:0")).toBe("Oxygen 49");
    expect(suggestedControllerName("Launchkey Mini")).toBe("Launchkey Mini");
    expect(suggestedControllerName("MPK mini 3 MIDI 1")).toBe("MPK mini 3");
    expect(suggestedControllerName("Client:Other port 20:1")).toBe("Other port");
    expect(suggestedControllerName("  ")).toBe("Controller");
  });

  it("learns each control once, by id or by message", () => {
    let inputs = withLearntInput([], knob);
    inputs = withLearntInput(inputs, knob);
    inputs = withLearntInput(inputs, { ...knob, id: "cc-1-74", name: "CC 74" });
    inputs = withLearntInput(inputs, button);
    expect(inputs.map((input) => input.id)).toEqual(["knob-1", "button-1"]);
  });

  it("is saved only when every control is named and fits what it sends", () => {
    expect(userControllerProblem("", [knob])).toMatch(/name/);
    expect(userControllerProblem("Oxygen", [])).toMatch(/at least one/);
    expect(userControllerProblem("Oxygen", [{ ...knob, name: " " }])).toMatch(/Every control/);
    expect(userControllerProblem("Oxygen", [{ ...knob, kind: "pad" }])).toMatch(/sends no note/);
    expect(userControllerProblem("Oxygen", [{ ...pad, kind: "knob" }])).toMatch(/no control change/);
    expect(userControllerProblem("Oxygen", [knob, pad, wheel])).toBeNull();
    expect(kindsForInput(pad)).toEqual(["pad", "button"]);
    expect(kindsForInput(wheel)).toEqual(["wheel"]);
    expect(isUserController("user.oxygen")).toBe(true);
    expect(isUserController("org.rackforge.keylab")).toBe(false);
  });
});

describe("how the editor lays things out", () => {
  it("keeps the package's groups in order, the ungrouped last", () => {
    const groups = groupInputs([knob, button, pad, { ...knob, id: "knob-2" }]);
    expect(groups.map((group) => [group.group, group.inputs.map((input) => input.id)])).toEqual([
      ["Knobs", ["knob-1", "knob-2"]],
      ["Buttons", ["button-1"]],
      ["Other", ["pad-1"]],
    ]);
    expect(groupInputs([pad]).map((group) => group.group)).toEqual(["Controls"]);
  });

  it("lists the parameters a controller can drive, found by any word", () => {
    const schema: PluginParameterSnapshot["schema"] = {
      schema_version: 1,
      pages: [
        { id: "voice", name: "Voice", order: 1 },
        { id: "fx", name: "Effects", order: 2 },
      ],
      parameters: [
        { ...leslie, page: "fx", group: "Rotary" },
        { ...drive, page: "voice" },
        { ...meter, page: "voice" },
        { ...reset, page: "voice", flags: { ...reset.flags, read_only: true } },
      ],
    };
    expect(parameterSections(schema).map((section) => [section.title, section.parameters.map((p) => p.id)])).toEqual([
      ["Voice", ["drive"]],
      ["Effects · Rotary", ["leslie.speed"]],
    ]);
    expect(parameterSections(schema, "rotary sp").flatMap((section) => section.parameters.map((p) => p.id))).toEqual([
      "leslie.speed",
    ]);
    expect(parameterSections(schema, "nothing")).toEqual([]);
  });

  it("names a package's standard meanings in words", () => {
    expect(roleLabel("synth.filter.cutoff")).toBe("Filter cutoff");
    expect(roleLabel("rackforge.master.level")).toBe("Master level");
    expect(roleLabel("synth.filter.lfo.amount")).toBe("Filter LFO amount");
    expect(standardMeaning(knob, [{ input: "knob-1", role: "synth.filter.cutoff" }])).toBe("Filter cutoff");
    expect(standardMeaning(button, [], [{ input: "button-1", target: "keyboard_parts" }])).toBe("keyboard parts");
    expect(standardMeaning(pad)).toBeNull();
  });
});
