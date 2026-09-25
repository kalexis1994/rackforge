import { describe, expect, it } from "vitest";
import {
  buildControllerDevices,
  type ControllerInput,
  defaultMode,
  describeMode,
  emptyControllerMap,
  fnCandidate,
  groupInputs,
  inputForActivity,
  inputFromActivity,
  inputMessageLabel,
  isModifierInput,
  isUserController,
  kindsForInput,
  learntInput,
  mappedInputFor,
  mappingsForInput,
  mappingsForParameter,
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
  withModifier,
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

  // A Launchkey MK4's Play and Stop send MIDI Start and Stop, which carry no
  // channel: they light their buttons, and are learnt as buttons.
  it("knows a transport button that sends a real time message", () => {
    const play: ControllerInput = { id: "play", name: "Play", kind: "button", midi: { realtime: "start" } };
    const stop: ControllerInput = { id: "stop", name: "Stop", kind: "button", midi: { realtime: "stop" } };
    expect(inputForActivity({ status: 0xfa, data1: 0 }, [...inputs, play, stop])).toBe(play);
    expect(inputForActivity({ status: 0xfc, data1: 0 }, [...inputs, play, stop])).toBe(stop);
    expect(inputForActivity({ status: 0xfb, data1: 0 }, [...inputs, play, stop])).toBeUndefined();
    expect(inputMessageLabel(play)).toBe("MIDI Start");
    expect(kindsForInput(play)).toEqual(["button"]);
    // No parameter link reads it: it drives the transport.
    expect(mappedInputFor(play)).toBeNull();
    expect(inputFromActivity({ status: 0xfa, data1: 0, data2: 0 })).toEqual({
      id: "realtime-start",
      name: "MIDI Start",
      kind: "button",
      midi: { realtime: "start" },
    });
    expect(inputFromActivity({ status: 0xf8, data1: 0, data2: 0 })).toBeNull();
  });
});

describe("what an input can do to a parameter", () => {
  it("offers buttons presses and knobs positions", () => {
    expect(modesFor(button, leslie)).toEqual(["cycle", "toggle", "set", "hold", "step"]);
    expect(modesFor(pad, reset)).toEqual(["trigger"]);
    expect(modesFor(knob, drive)).toEqual(["direct", "range"]);
    expect(modesFor(knob, leslie)).toEqual(["zones", "direct"]);
    expect(modesFor(knob, meter)).toEqual([]);
    // An encoder that sends how far it turned never falls into a zone.
    const encoder: ControllerInput = {
      id: "encoder-1",
      name: "Encoder 1",
      kind: "encoder",
      encoder: "relative_twos_complement",
      midi: { channel: 0, cc: 24 },
    };
    expect(modesFor(encoder, leslie)).toEqual(["direct"]);
    expect(modesFor(encoder, drive)).toEqual(["direct", "range"]);
    expect(mappedInputFor(encoder)?.relative).toBe("twos_complement");
    expect(mappedInputFor(knob)).not.toHaveProperty("relative");
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

  it("gives an input one more thing to do with Fn", () => {
    let map = withMapping(emptyControllerMap("x", "X"), organ, mapping("a", knob, "drive"));
    map = withMapping(map, organ, { ...mapping("b", knob, "leslie.speed"), layer: "fn" });
    expect(map.plugins[0].mappings.map((m) => m.id)).toEqual(["a", "b"]);
    // Again with Fn: it replaces only what the knob did with Fn.
    map = withMapping(map, organ, { ...mapping("c", knob, "percussion"), layer: "fn" });
    expect(map.plugins[0].mappings.map((m) => m.id)).toEqual(["a", "c"]);
    map = withMapping(map, organ, mapping("d", knob, "chorus"));
    expect(map.plugins[0].mappings.map((m) => m.id)).toEqual(["c", "d"]);
  });

  it("picks the Fn button from a press, and refuses what cannot be one", () => {
    const play: ControllerInput = { id: "play", name: "Play", kind: "button", midi: { realtime: "start" } };
    const inputs = [knob, button, pad, play];
    expect(fnCandidate({ status: 0xb0, data1: 20, data2: 127 }, inputs)).toEqual({ input: button });
    expect(fnCandidate({ status: 0x99, data1: 36, data2: 90 }, inputs)).toEqual({ input: pad });
    // Releases choose nothing: the press already did, or will.
    expect(fnCandidate({ status: 0xb0, data1: 20, data2: 0 }, inputs)).toBeNull();
    expect(fnCandidate({ status: 0x89, data1: 36, data2: 0 }, inputs)).toBeNull();
    expect(fnCandidate({ status: 0x99, data1: 36, data2: 0 }, inputs)).toBeNull();
    expect(fnCandidate({ status: 0xb0, data1: 74, data2: 64 }, inputs)).toEqual({
      problem: "Knob 1 is not a button. Press a button or a pad.",
    });
    expect(fnCandidate({ status: 0xfa, data1: 0, data2: 0 }, inputs)).toEqual({
      problem: "Play sends MIDI Start, which cannot open a layer.",
    });
    expect(fnCandidate({ status: 0xb3, data1: 99, data2: 127 }, inputs)).toEqual({
      problem: "CC 99 · ch 4 is none of this controller's controls.",
    });
  });

  it("makes a button the Fn button, which then does nothing else", () => {
    let map = withMapping(emptyControllerMap("x", "X"), organ, mapping("a", button, "leslie.speed"));
    map = withMapping(map, organ, mapping("b", knob, "drive"));
    map = withModifier(map, button);
    expect(map.modifier).toEqual({ input: mappedInputFor(button), mode: "hold_or_double_tap" });
    expect(map.plugins[0].mappings.map((m) => m.id)).toEqual(["b"]);
    expect(isModifierInput(map, button)).toBe(true);
    // The same button, learnt under another name, is still the Fn button.
    expect(isModifierInput(map, { ...button, id: "cc-1-20", name: "CC 20" })).toBe(true);
    expect(isModifierInput(map, knob)).toBe(false);

    const toggled = withModifier(map, pad, "toggle");
    expect(toggled.modifier?.input.id).toBe("pad-1");
    expect(toggled.modifier?.mode).toBe("toggle");
    expect(withModifier(toggled, null).modifier).toBeUndefined();
    expect(withModifier(toggled, null).plugins).toEqual(toggled.plugins);
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

  it("shows a catalog keyboard only once it is plugged in or mapped", () => {
    const catalog = { ...keylab, runtime: "DeclarativeV1" };
    const launchkey = { ...catalog, id: "org.rackforge.novation-launchkey-mk3-49", name: "Launchkey 49 [MK3]" };
    const minilab = { ...catalog, id: "org.rackforge.arturia-minilab-3", name: "MiniLab 3" };
    const oxygenPro = { ...catalog, id: "org.rackforge.m-audio-oxygen-pro-49", name: "Oxygen Pro 49" };
    const devices = buildControllerDevices(
      [launchkey, minilab, oxygenPro, keylab],
      [{ controller_id: launchkey.id, source: { id: "alsa.lk", name: "Launchkey MK3 49" }, connected: true }],
      [emptyControllerMap(minilab.id, minilab.name)],
    );
    expect(devices.map((device) => device.id)).toEqual([launchkey.id, keylab.id, minilab.id]);
  });

  it("does not list a catalog keyboard for its factory map alone", () => {
    const catalog = { ...keylab, runtime: "DeclarativeV1" };
    const launchkey = { ...catalog, id: "org.rackforge.novation-launchkey-mk3-49", name: "Launchkey 49 [MK3]" };
    const flkey = { ...catalog, id: "org.rackforge.novation-flkey-49", name: "FLkey 49" };
    const mini = { ...catalog, id: "org.rackforge.novation-launchkey-mini-mk3", name: "Launchkey Mini [MK3]" };
    const maps = [launchkey, flkey, mini, keylab].map((entry) => emptyControllerMap(entry.id, entry.name));
    const devices = buildControllerDevices(
      [launchkey, flkey, mini, keylab],
      [{ controller_id: mini.id, source: { id: "alsa.mini", name: "Launchkey Mini MK3 MIDI" }, connected: true }],
      maps,
      // The FLkey's map is the player's; the others are as RackForge offered
      // them. The KeyLab is not a catalog package: it shows either way.
      new Set([launchkey.id, mini.id, keylab.id]),
    );
    expect(devices.map((device) => device.id).sort()).toEqual([flkey.id, keylab.id, mini.id].sort());
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

describe("a control learnt from a plugin's own panel", () => {
  it("is the package's control when one sends the message", () => {
    const found = learntInput({ type: "control_change", controller: 20 }, 1, [knob, button]);
    expect(found.kind).toBe("button");
    expect(found.input.id).toBe("button-1");
  });

  it("is named after the message otherwise", () => {
    expect(learntInput({ type: "control_change", controller: 21 }, 2)).toEqual({
      input: {
        id: "cc-2-21",
        name: "CC 21",
        channel: { mode: "channel", channel: 2 },
        message: { type: "control_change", controller: 21 },
      },
      kind: "knob",
    });
    expect(learntInput({ type: "note", note: 40 }, 10).kind).toBe("pad");
    expect(learntInput({ type: "pitch_bend" }, 1).input.id).toBe("bend-1");
  });

  it("finds what already drives the parameter, in every map", () => {
    const organ = { plugin_id: "org.rackforge.organ", plugin_name: "RF-Organ" };
    const mapped = { id: "a", input: mappedInputFor(button)!, parameter_id: "leslie.speed", mode: { kind: "trigger" } } as const;
    const maps = [
      withMapping(emptyControllerMap("user.a", "A"), organ, { ...mapped }),
      withMapping(emptyControllerMap("user.b", "B"), organ, { ...mapped, id: "b", parameter_id: "drive" }),
    ];
    expect(mappingsForParameter(maps, organ.plugin_id, "leslie.speed").map((entry) => entry.controller_id)).toEqual([
      "user.a",
    ]);
    expect(mappingsForParameter(maps, "other", "leslie.speed")).toEqual([]);
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
