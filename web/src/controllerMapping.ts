/**
 * What the Controllers editor knows about a controller and does to a map:
 * the controller's inputs, which of them a MIDI message comes from, which
 * modes an input can drive a parameter with, and how a map changes.
 *
 * Pure functions over plain data, so every rule the editor follows is
 * tested here rather than found out on stage. The host checks a map again
 * when it is saved (rackforge-midi-api `ControllerMap::validate`, and each
 * mode against its parameter when compiled); these rules are the same ones,
 * applied before the player is shown an error.
 */
import type {
  ControlMapping,
  ControllerMap,
  MapLayer,
  MidiActivityEvent,
  ModifierMode,
  ParameterLinkMessage,
  ParameterLinkMode,
  PluginParameterDescriptor,
  PluginParameterSnapshot,
  RelativeEncoding,
} from "./types";

export type InputKind = "knob" | "fader" | "encoder" | "button" | "pad" | "wheel" | "pedal";

/** The MIDI System Real Time messages some keyboards' transport buttons send. */
export type RealtimeMessage = "start" | "continue" | "stop";

const REALTIME_STATUS: Record<RealtimeMessage, number> = { start: 0xfa, continue: 0xfb, stop: 0xfc };
const REALTIME_LABEL: Record<RealtimeMessage, string> = {
  start: "MIDI Start",
  continue: "MIDI Continue",
  stop: "MIDI Stop",
};

function realtimeOf(status: number): RealtimeMessage | undefined {
  return (Object.keys(REALTIME_STATUS) as RealtimeMessage[]).find((message) => REALTIME_STATUS[message] === status);
}

/** One physical control, as a controller package declares it. */
export interface ControllerInput {
  id: string;
  name: string;
  kind: InputKind;
  group?: string;
  /**
   * Exactly one message: a control change, note or pitch bend on a
   * zero-based channel, or a real time message, which has no channel.
   */
  midi: { channel?: number; cc?: number; note?: number; pitch_bend?: boolean; realtime?: RealtimeMessage };
  button?: { press?: number; release?: number; press_only?: boolean; latching?: boolean };
  encoder?: string;
}

export interface ControllerRole {
  input: string;
  role: string;
  invert?: boolean;
  mode?: "absolute" | "relative";
}

export interface ControllerAction {
  input: string;
  target: string | Record<string, unknown>;
}

/** A controller package as `/api/v1/controllers` lists it. */
export interface ControllerPackageSummary {
  id: string;
  name: string;
  vendor?: string | null;
  schema_version?: number;
  version: string;
  enabled: boolean;
  trust: string;
  runtime: string;
  inputs?: ControllerInput[];
  roles?: ControllerRole[];
  actions?: ControllerAction[];
  /** What the package sends to its controller, and whether it may. */
  output?: ControllerOutput;
}

/**
 * A package's messages to its controller: none, asked for and waiting on
 * the player, or allowed (by the player, or because RackForge ships it).
 */
export interface ControllerOutput {
  state: "none" | "asked" | "allowed";
  messages: string[];
  sysex: boolean;
}

export type ModeKind = ParameterLinkMode["kind"];
export type ParameterSchema = PluginParameterSnapshot["schema"];

const BUTTON_KINDS: ReadonlySet<InputKind> = new Set(["button", "pad"]);

export function isButtonInput(input: Pick<ControllerInput, "kind">): boolean {
  return BUTTON_KINDS.has(input.kind);
}

/** The message an input sends, as a link names it. */
export function inputMessage(input: ControllerInput): ParameterLinkMessage | null {
  if (typeof input.midi.cc === "number") return { type: "control_change", controller: input.midi.cc };
  if (typeof input.midi.note === "number") return { type: "note", note: input.midi.note };
  if (input.midi.pitch_bend) return { type: "pitch_bend" };
  return null;
}

const RELATIVE_ENCODINGS: Record<string, RelativeEncoding> = {
  relative_twos_complement: "twos_complement",
  relative_binary_offset: "binary_offset",
  relative_sign_magnitude: "sign_magnitude",
};

/** How an endless encoder sends its turns, when it sends how far rather than where. */
export function relativeEncoding(input: Pick<ControllerInput, "kind" | "encoder">): RelativeEncoding | undefined {
  return input.kind === "encoder" && input.encoder ? RELATIVE_ENCODINGS[input.encoder] : undefined;
}

/**
 * The input as a mapping records it: its id, its name, and a copy of its
 * message -- and, for an encoder that sends how far it turned, how it says so.
 */
export function mappedInputFor(input: ControllerInput): ControlMapping["input"] | null {
  const message = inputMessage(input);
  if (!message || input.midi.channel === undefined) return null;
  const relative = relativeEncoding(input);
  return {
    id: input.id,
    name: input.name,
    // Links name channels from 1, packages from 0.
    channel: { mode: "channel", channel: input.midi.channel + 1 },
    message,
    ...(relative ? { relative } : {}),
  };
}

/** Which input, if any, a received message comes from. */
export function inputForActivity(
  event: Pick<MidiActivityEvent, "status" | "data1">,
  inputs: readonly ControllerInput[],
): ControllerInput | undefined {
  const realtime = realtimeOf(event.status);
  if (realtime) return inputs.find((input) => input.midi.realtime === realtime);
  const kind = event.status & 0xf0;
  const channel = event.status & 0x0f;
  return inputs.find((input) => {
    if (input.midi.channel !== channel) return false;
    if (kind === 0xb0) return input.midi.cc === event.data1;
    if (kind === 0x90 || kind === 0x80) return input.midi.note === event.data1;
    if (kind === 0xe0) return input.midi.pitch_bend === true;
    return false;
  });
}

/**
 * An input made from a message, for a controller RackForge does not know:
 * a control change is taken for a knob, a note for a pad, a bend for the
 * wheel. The player renames it and changes its kind; the id stays, keyed to
 * the message so learning the same control twice finds it again.
 */
export function inputFromActivity(
  event: Pick<MidiActivityEvent, "status" | "data1" | "data2">,
): ControllerInput | null {
  const realtime = realtimeOf(event.status);
  if (realtime) {
    // A transport button: its Play or Stop sends the real time message.
    return { id: `realtime-${realtime}`, name: REALTIME_LABEL[realtime], kind: "button", midi: { realtime } };
  }
  const kind = event.status & 0xf0;
  const channel = event.status & 0x0f;
  const user = channel + 1;
  if (kind === 0xb0) {
    return {
      id: `cc-${user}-${event.data1}`,
      name: `CC ${event.data1}`,
      kind: "knob",
      midi: { channel, cc: event.data1 },
    };
  }
  // A note-off, or a note-on at velocity 0, is the end of a press: the
  // press that came before it already made the input.
  if (kind === 0x90 && event.data2 > 0) {
    return {
      id: `note-${user}-${event.data1}`,
      name: `Note ${event.data1}`,
      kind: "pad",
      midi: { channel, note: event.data1 },
    };
  }
  if (kind === 0xe0) {
    return { id: `bend-${user}`, name: "Pitch wheel", kind: "wheel", midi: { channel, pitch_bend: true } };
  }
  return null;
}

/** What a control sends, as a player can check it on the hardware. */
export function inputMessageLabel(input: ControllerInput): string {
  if (input.midi.realtime) return REALTIME_LABEL[input.midi.realtime];
  const channel = `ch ${(input.midi.channel ?? 0) + 1}`;
  if (typeof input.midi.cc === "number") return `CC ${input.midi.cc} · ${channel}`;
  if (typeof input.midi.note === "number") return `Note ${input.midi.note} · ${channel}`;
  if (input.midi.pitch_bend) return `Pitch bend · ${channel}`;
  return channel;
}

/** Inputs in the package's groups, in order; ungrouped ones last. */
export function groupInputs(inputs: readonly ControllerInput[]): Array<{ group: string; inputs: ControllerInput[] }> {
  const groups: Array<{ group: string; inputs: ControllerInput[] }> = [];
  const ungrouped: ControllerInput[] = [];
  for (const input of inputs) {
    if (!input.group) {
      ungrouped.push(input);
      continue;
    }
    const existing = groups.find((entry) => entry.group === input.group);
    if (existing) existing.inputs.push(input);
    else groups.push({ group: input.group, inputs: [input] });
  }
  if (ungrouped.length > 0) groups.push({ group: groups.length > 0 ? "Other" : "Controls", inputs: ungrouped });
  return groups;
}

/** A parameter a controller may drive: not read-only, not a meter. */
export function isMappableParameter(parameter: PluginParameterDescriptor): boolean {
  return !parameter.flags.read_only && parameter.kind.type !== "meter";
}

/**
 * The mappable parameters, by page and group, filtered by what the player
 * typed: every word must appear in the name, the group, the page or the id.
 */
export function parameterSections(
  schema: ParameterSchema,
  query = "",
): Array<{ title: string; parameters: PluginParameterDescriptor[] }> {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const pageName = new Map(schema.pages.map((page) => [page.id, page.name]));
  const pageOrder = new Map(schema.pages.map((page) => [page.id, page.order]));
  const matching = schema.parameters
    .filter(isMappableParameter)
    .filter((parameter) => {
      const haystack = [parameter.name, parameter.group ?? "", pageName.get(parameter.page) ?? "", parameter.id]
        .join(" ")
        .toLowerCase();
      return words.every((word) => haystack.includes(word));
    })
    .sort((left, right) =>
      (pageOrder.get(left.page) ?? 0) - (pageOrder.get(right.page) ?? 0)
      || (left.group ?? "").localeCompare(right.group ?? "")
      || left.order - right.order,
    );
  const sections: Array<{ title: string; parameters: PluginParameterDescriptor[] }> = [];
  for (const parameter of matching) {
    const page = pageName.get(parameter.page) ?? parameter.page;
    const title = parameter.group ? `${page} · ${parameter.group}` : page;
    const last = sections[sections.length - 1];
    if (last && last.title === title) last.parameters.push(parameter);
    else sections.push({ title, parameters: [parameter] });
  }
  return sections;
}

/** The modes an input can drive a parameter with, the most natural first. */
export function modesFor(
  input: Pick<ControllerInput, "kind" | "encoder">,
  parameter: PluginParameterDescriptor,
): ModeKind[] {
  const kind = parameter.kind.type;
  // An encoder that sends how far it turned moves the parameter from where
  // it is: across its range or a part of it, never to a zone.
  if (relativeEncoding(input)) {
    switch (kind) {
      case "float":
      case "integer":
        return ["direct", "range"];
      case "enum":
      case "boolean":
        return ["direct"];
      default:
        return [];
    }
  }
  if (isButtonInput(input)) {
    switch (kind) {
      case "enum":
        return ["cycle", "toggle", "set", "hold", "step"];
      case "boolean":
        return ["toggle", "set", "hold"];
      case "float":
      case "integer":
        return ["toggle", "set", "hold", "step"];
      case "trigger":
        return ["trigger"];
      default:
        return [];
    }
  }
  switch (kind) {
    case "float":
    case "integer":
      return ["direct", "range"];
    case "enum":
      return ["zones", "direct"];
    case "boolean":
    case "trigger":
      return ["direct"];
    default:
      return [];
  }
}

/** The values a parameter accepts, when it has a list of them. */
export function parameterChoices(parameter: PluginParameterDescriptor): Array<{ value: number; name: string }> {
  switch (parameter.kind.type) {
    case "enum":
      return parameter.kind.choices;
    case "boolean":
      return [{ value: 0, name: "Off" }, { value: 1, name: "On" }];
    default:
      return [];
  }
}

function range(parameter: PluginParameterDescriptor): { minimum: number; maximum: number } | null {
  return parameter.kind.type === "float" || parameter.kind.type === "integer"
    ? { minimum: parameter.kind.minimum, maximum: parameter.kind.maximum }
    : null;
}

/** A mode of this kind, filled in with sensible values for the parameter. */
export function defaultMode(kind: ModeKind, parameter: PluginParameterDescriptor): ParameterLinkMode {
  const choices = parameterChoices(parameter);
  const span = range(parameter);
  const low = span?.minimum ?? choices[0]?.value ?? 0;
  const high = span?.maximum ?? choices[choices.length - 1]?.value ?? 1;
  const second = choices[1]?.value ?? high;
  switch (kind) {
    case "direct":
      return { kind: "direct" };
    case "range":
      return { kind: "range", min: low, max: high };
    case "zones":
      return { kind: "zones", values: choices.length >= 2 ? choices.map((choice) => choice.value) : [low, high] };
    case "set":
      return { kind: "set", value: choices.length > 1 ? second : high };
    case "toggle":
      return { kind: "toggle", first: choices.length > 1 ? choices[0].value : low, second: choices.length > 1 ? second : high };
    case "cycle":
      return { kind: "cycle", values: choices.length >= 2 ? choices.map((choice) => choice.value) : [low, high] };
    case "hold":
      return { kind: "hold", pressed: choices.length > 1 ? second : high, released: choices.length > 1 ? choices[0].value : low };
    case "step":
      return { kind: "step", direction: "up", wrap: false };
    case "trigger":
      return { kind: "trigger" };
  }
}

/** The first mode an input offers for a parameter, filled in. */
export function suggestedMode(
  input: Pick<ControllerInput, "kind">,
  parameter: PluginParameterDescriptor,
): ParameterLinkMode | null {
  const [first] = modesFor(input, parameter);
  return first ? defaultMode(first, parameter) : null;
}

export const MODE_LABELS: Record<ModeKind, string> = {
  direct: "Direct",
  range: "Range",
  zones: "Zones",
  set: "Set",
  toggle: "Toggle",
  cycle: "Cycle",
  hold: "Hold",
  step: "Step",
  trigger: "Trigger",
};

export const MODE_HINTS: Record<ModeKind, string> = {
  direct: "The control's travel spans the whole parameter.",
  range: "The control's travel spans the two values you choose.",
  zones: "The control's travel is split among the values, in order.",
  set: "Each press sets one value.",
  toggle: "Each press switches between two values.",
  cycle: "Each press moves to the next value, back to the first after the last.",
  hold: "One value while held, another when let go.",
  step: "Each press moves the parameter one step.",
  trigger: "Each press fires the parameter.",
};

/** A value as the player reads it: a choice's name, or the number with its unit. */
export function valueLabel(parameter: PluginParameterDescriptor, value: number): string {
  const choice = parameterChoices(parameter).find((candidate) => candidate.value === value);
  if (choice) return choice.name;
  const unit = parameter.kind.type === "float" || parameter.kind.type === "integer" ? parameter.kind.unit : undefined;
  const text = Number.isInteger(value) ? String(value) : String(Number(value.toFixed(3)));
  return unit ? `${text} ${unit}` : text;
}

/** One line for a mapping: "Toggle · Chorale ↔ Tremolo". */
export function describeMode(mode: ParameterLinkMode | undefined, parameter?: PluginParameterDescriptor): string {
  const label = (value: number) => (parameter ? valueLabel(parameter, value) : String(value));
  const current = mode ?? { kind: "direct" as const };
  switch (current.kind) {
    case "direct":
      return "Direct";
    case "range":
      return `Range · ${label(current.min)} – ${label(current.max)}`;
    case "zones":
      return `Zones · ${current.values.map(label).join(" / ")}`;
    case "set":
      return `Set · ${label(current.value)}`;
    case "toggle":
      return `Toggle · ${label(current.first)} ↔ ${label(current.second)}`;
    case "cycle":
      return `Cycle · ${current.values.map(label).join(" → ")}`;
    case "hold":
      return `Hold · ${label(current.pressed)}, then ${label(current.released)}`;
    case "step":
      return `Step ${current.direction === "up" ? "up" : "down"}${current.wrap ? " · wraps" : ""}`;
    case "trigger":
      return "Trigger";
  }
}

function acceptsValue(parameter: PluginParameterDescriptor, value: number): boolean {
  if (!Number.isFinite(value)) return false;
  const choices = parameterChoices(parameter);
  if (choices.length > 0) return choices.some((choice) => choice.value === value);
  const span = range(parameter);
  if (span) {
    if (value < span.minimum || value > span.maximum) return false;
    return parameter.kind.type !== "integer" || Number.isInteger(value);
  }
  return false;
}

/**
 * Why a mode cannot drive this parameter from this input, or null when it
 * can: the same checks the host makes when it compiles the map.
 */
export function modeProblem(
  input: Pick<ControllerInput, "kind">,
  parameter: PluginParameterDescriptor,
  mode: ParameterLinkMode,
): string | null {
  if (!isMappableParameter(parameter)) return `${parameter.name} cannot be driven by a controller.`;
  if (!modesFor(input, parameter).includes(mode.kind)) {
    return `${MODE_LABELS[mode.kind]} does not fit a ${input.kind} on ${parameter.name}.`;
  }
  const values: number[] = (() => {
    switch (mode.kind) {
      case "range":
        return [mode.min, mode.max];
      case "zones":
      case "cycle":
        return mode.values;
      case "set":
        return [mode.value];
      case "toggle":
        return [mode.first, mode.second];
      case "hold":
        return [mode.pressed, mode.released];
      default:
        return [];
    }
  })();
  const refused = values.find((value) => !acceptsValue(parameter, value));
  if (refused !== undefined) return `${parameter.name} does not accept ${refused}.`;
  if ((mode.kind === "zones" || mode.kind === "cycle") && mode.values.length < 2) {
    return "Choose at least two values.";
  }
  if ((mode.kind === "zones" || mode.kind === "cycle") && mode.values.length > 128) {
    return "Choose at most 128 values.";
  }
  if (mode.kind === "range" && mode.min === mode.max) return "The two ends of a range must differ.";
  if (mode.kind === "toggle" && mode.first === mode.second) return "A toggle switches between two different values.";
  return null;
}

/** A fresh id for a mapping: a link identifier the host accepts. */
export function newMappingId(random: () => number = Math.random): string {
  const part = () => Math.floor(random() * 36 ** 6).toString(36).padStart(6, "0");
  return `m-${part()}${part()}`;
}

export function emptyControllerMap(controllerId: string, controllerName: string): ControllerMap {
  return { schema_version: 1, controller_id: controllerId, controller_name: controllerName, plugins: [] };
}

function sameMessage(left: ControlMapping["input"], right: ControlMapping["input"]): boolean {
  return JSON.stringify(left.message) === JSON.stringify(right.message)
    && JSON.stringify(left.channel) === JSON.stringify(right.channel);
}

/** The layer a mapping acts in: the base one unless it says Fn. */
export function mappingLayer(mapping: ControlMapping): MapLayer {
  return mapping.layer ?? "base";
}

/**
 * The map with this mapping in place of whatever the same input -- by id or
 * by message -- did in the same plugin and layer: an input does one thing in
 * each plugin, and one more with Fn. The map is not changed; a new one is
 * returned.
 */
export function withMapping(
  map: ControllerMap,
  plugin: { plugin_id: string; plugin_name: string },
  mapping: ControlMapping,
): ControllerMap {
  const others = map.plugins.filter((entry) => entry.plugin_id !== plugin.plugin_id);
  const existing = map.plugins.find((entry) => entry.plugin_id === plugin.plugin_id);
  const layer = mappingLayer(mapping);
  const kept = (existing?.mappings ?? []).filter(
    (candidate) =>
      candidate.id !== mapping.id
      && (mappingLayer(candidate) !== layer
        || (candidate.input.id !== mapping.input.id && !sameMessage(candidate.input, mapping.input))),
  );
  return {
    ...map,
    plugins: [...others, { plugin_id: plugin.plugin_id, plugin_name: plugin.plugin_name, mappings: [...kept, mapping] }],
  };
}

/**
 * The map with this input as its Fn button, or with none. The button opens
 * the Fn layer and does nothing else, so what it was mapped to goes.
 */
export function withModifier(
  map: ControllerMap,
  input: ControllerInput | null,
  mode: ModifierMode = "hold_or_double_tap",
): ControllerMap {
  if (!input) {
    const rest = { ...map };
    delete rest.modifier;
    return rest;
  }
  const mapped = mappedInputFor(input);
  if (!mapped) return map;
  return {
    ...map,
    modifier: { input: mapped, mode },
    plugins: map.plugins
      .map((entry) => ({
        ...entry,
        mappings: entry.mappings.filter(
          (mapping) => mapping.input.id !== input.id && !sameMessage(mapping.input, mapped),
        ),
      }))
      .filter((entry) => entry.mappings.length > 0),
  };
}

/** Whether an input is its controller's Fn button. */
export function isModifierInput(map: ControllerMap | undefined, input: ControllerInput): boolean {
  const modifier = map?.modifier;
  if (!modifier) return false;
  const mapped = mappedInputFor(input);
  return modifier.input.id === input.id || (mapped !== null && sameMessage(modifier.input, mapped));
}

/**
 * What a message heard while the player picks an Fn button says: the button
 * pressed, a reason it cannot be one, or nothing (a release, a knob's
 * travel already refused). Only a press chooses, so the release of the
 * button that opened the choice never does.
 */
export type FnCandidate = { input: ControllerInput } | { problem: string };

export function fnCandidate(
  event: Pick<MidiActivityEvent, "status" | "data1" | "data2">,
  inputs: readonly ControllerInput[],
): FnCandidate | null {
  const kind = event.status & 0xf0;
  const input = inputForActivity(event, inputs);
  if (!input) {
    const unknown = inputFromActivity(event);
    return unknown ? { problem: `${inputMessageLabel(unknown)} is none of this controller's controls.` } : null;
  }
  if (!isButtonInput(input)) return { problem: `${input.name} is not a button. Press a button or a pad.` };
  if (!mappedInputFor(input)) {
    return { problem: `${input.name} sends ${inputMessageLabel(input)}, which cannot open a layer.` };
  }
  const pressed = kind === 0x90 ? event.data2 > 0 : kind === 0xb0 ? event.data2 > 0 : false;
  return pressed ? { input } : null;
}

/** The map without one mapping; a plugin left with none is dropped. */
export function withoutMapping(map: ControllerMap, pluginId: string, mappingId: string): ControllerMap {
  return {
    ...map,
    plugins: map.plugins
      .map((entry) =>
        entry.plugin_id === pluginId
          ? { ...entry, mappings: entry.mappings.filter((mapping) => mapping.id !== mappingId) }
          : entry,
      )
      .filter((entry) => entry.mappings.length > 0),
  };
}

/** Every mapping an input carries, plugin by plugin. */
export function mappingsForInput(
  map: ControllerMap | undefined,
  input: ControllerInput,
): Array<{ plugin_id: string; plugin_name: string; mapping: ControlMapping }> {
  if (!map) return [];
  const mapped = mappedInputFor(input);
  return map.plugins.flatMap((plugin) =>
    plugin.mappings
      .filter((mapping) => mapping.input.id === input.id || (mapped !== null && sameMessage(mapping.input, mapped)))
      .map((mapping) => ({ plugin_id: plugin.plugin_id, plugin_name: plugin.plugin_name, mapping })),
  );
}

/** The package's standard meaning for an input: a role or a host action. */
export function standardMeaning(
  input: ControllerInput,
  roles: readonly ControllerRole[] = [],
  actions: readonly ControllerAction[] = [],
): string | null {
  const role = roles.find((candidate) => candidate.input === input.id);
  if (role) return roleLabel(role.role);
  const action = actions.find((candidate) => candidate.input === input.id);
  if (action) {
    const target = typeof action.target === "string" ? action.target : Object.keys(action.target)[0] ?? "action";
    return target.replace(/_/g, " ");
  }
  return null;
}

/** A controller as the editor shows it: package, attachment and map together. */
export interface ControllerDevice {
  id: string;
  name: string;
  vendor?: string;
  /** The installed package, when there is one. */
  package?: ControllerPackageSummary;
  /** The input the host attached it to, when it did. */
  source?: { id: string; name: string };
  connected: boolean;
  /** Recognised by the device's own Identity Reply. */
  identified?: boolean;
  map?: ControllerMap;
  inputs: ControllerInput[];
  roles: ControllerRole[];
  actions: ControllerAction[];
  /** Inputs known only from the map: the package that named them is gone. */
  orphaned: boolean;
  /** An enabled input no package recognises: its controls are learnt here. */
  unknown?: boolean;
}

/**
 * A controller's name from the name of the input it was heard on: ALSA's
 * "Oxygen 49:Oxygen 49 MIDI 1 24:0" is "Oxygen 49", the port numbers and
 * the repeated client name left out.
 */
export function suggestedControllerName(sourceName: string): string {
  let name = sourceName.trim().replace(/\s+\d+:\d+$/, "");
  const colon = name.indexOf(":");
  if (colon > 0) {
    const client = name.slice(0, colon).trim();
    const port = name.slice(colon + 1).trim();
    name = port.toLowerCase().startsWith(client.toLowerCase()) ? client : port || client;
  }
  name = name.replace(/\s+MIDI(\s*\d+)?$/i, "").trim();
  return name || sourceName.trim() || "Controller";
}

/**
 * The input a learnt message is, as a mapping records it: the package's own
 * control when one sends that message, or one named after the message.
 * The kind says which modes it can take; a control change nobody named is
 * taken for a knob, and the player may say it is a button.
 */
export function learntInput(
  message: ParameterLinkMessage,
  channel: number,
  packageInputs: readonly ControllerInput[] = [],
): { input: ControlMapping["input"]; kind: InputKind } {
  const heard: ControlMapping["input"] = {
    id: "",
    name: "",
    channel: { mode: "channel", channel },
    message,
  };
  for (const candidate of packageInputs) {
    const mapped = mappedInputFor(candidate);
    if (mapped && sameMessage(mapped, heard)) return { input: mapped, kind: candidate.kind };
  }
  switch (message.type) {
    case "control_change":
      return {
        input: { ...heard, id: `cc-${channel}-${message.controller}`, name: `CC ${message.controller}` },
        kind: "knob",
      };
    case "note":
      return { input: { ...heard, id: `note-${channel}-${message.note}`, name: `Note ${message.note}` }, kind: "pad" };
    case "pitch_bend":
      return { input: { ...heard, id: `bend-${channel}`, name: "Pitch wheel" }, kind: "wheel" };
    case "channel_pressure":
      return { input: { ...heard, id: `pressure-${channel}`, name: "Pressure" }, kind: "knob" };
    case "poly_pressure":
      return {
        input: { ...heard, id: `poly-${channel}-${message.note}`, name: `Pressure ${message.note}` },
        kind: "knob",
      };
  }
}

/** Every mapping, in every controller's map, that drives one parameter of one plugin. */
export function mappingsForParameter(
  maps: readonly ControllerMap[],
  pluginId: string,
  parameterId: string,
): Array<{ controller_id: string; controller_name: string; mapping: ControlMapping }> {
  return maps.flatMap((map) =>
    (map.plugins.find((plugin) => plugin.plugin_id === pluginId)?.mappings ?? [])
      .filter((mapping) => mapping.parameter_id === parameterId)
      .map((mapping) => ({ controller_id: map.controller_id, controller_name: map.controller_name, mapping })),
  );
}

/** Package ids a player made, which this editor may save again. */
export function isUserController(id: string): boolean {
  return id.startsWith("user.");
}

/**
 * The enabled MIDI inputs no controller package claims, as devices whose
 * controls are the ones learnt from them so far.
 */
export function unknownSourceDevices(
  sources: ReadonlyArray<{ source: { id: string; name: string }; connected: boolean }>,
  claimedSourceIds: ReadonlySet<string>,
  learnt: ReadonlyMap<string, ControllerInput[]>,
): ControllerDevice[] {
  return sources
    .filter((entry) => !claimedSourceIds.has(entry.source.id))
    .map((entry) => ({
      id: `midi:${entry.source.id}`,
      name: suggestedControllerName(entry.source.name),
      source: { id: entry.source.id, name: entry.source.name },
      connected: entry.connected,
      inputs: learnt.get(entry.source.id) ?? [],
      roles: [],
      actions: [],
      orphaned: false,
      unknown: true,
    }));
}

/**
 * The list with an input heard for the first time added at the end; one
 * already there, by id or by message, leaves the list as it is.
 */
export function withLearntInput(inputs: readonly ControllerInput[], input: ControllerInput): ControllerInput[] {
  const heard = mappedInputFor(input);
  const known = inputs.some((candidate) => {
    if (candidate.id === input.id) return true;
    const other = mappedInputFor(candidate);
    return heard !== null && other !== null && sameMessage(other, heard);
  });
  return known ? [...inputs] : [...inputs, input];
}

/** Why a set of controls cannot be saved as a controller, or null. */
export function userControllerProblem(name: string, inputs: readonly ControllerInput[]): string | null {
  if (!name.trim()) return "Give the controller a name.";
  if (name.trim().length > 64) return "Keep the name under 64 characters.";
  if (inputs.length === 0) return "Move at least one control so there is something to save.";
  const names = inputs.map((input) => input.name.trim());
  if (names.some((entry) => !entry || entry.length > 48)) return "Every control needs a name under 48 characters.";
  const pads = inputs.filter((input) => input.kind === "pad" && typeof input.midi.note !== "number");
  if (pads.length > 0) return `${pads[0].name} sends no note, so it cannot be a pad.`;
  const continuous = inputs.filter(
    (input) =>
      (input.kind === "knob" || input.kind === "fader" || input.kind === "encoder" || input.kind === "pedal")
      && typeof input.midi.cc !== "number",
  );
  if (continuous.length > 0) return `${continuous[0].name} sends no control change, so it cannot be a ${continuous[0].kind}.`;
  const wheels = inputs.filter(
    (input) => input.kind === "wheel" && !input.midi.pitch_bend && typeof input.midi.cc !== "number",
  );
  if (wheels.length > 0) return `${wheels[0].name} cannot be a wheel.`;
  return null;
}

/** The kinds a control can be declared as, given the message it sends. */
export function kindsForInput(input: Pick<ControllerInput, "midi">): InputKind[] {
  if (input.midi.realtime) return ["button"];
  if (typeof input.midi.note === "number") return ["pad", "button"];
  if (input.midi.pitch_bend) return ["wheel"];
  return ["knob", "fader", "encoder", "button", "pedal", "wheel"];
}

/** An input rebuilt from a mapping's copy of it, for a map whose package is gone. */
function inputFromMapped(input: ControlMapping["input"]): ControllerInput {
  const channel = input.channel.mode === "channel" ? input.channel.channel - 1 : 0;
  switch (input.message.type) {
    case "note":
      return { id: input.id, name: input.name, kind: "pad", midi: { channel, note: input.message.note } };
    case "pitch_bend":
      return { id: input.id, name: input.name, kind: "wheel", midi: { channel, pitch_bend: true } };
    case "control_change":
      return { id: input.id, name: input.name, kind: "knob", midi: { channel, cc: input.message.controller } };
    default:
      return { id: input.id, name: input.name, kind: "knob", midi: { channel } };
  }
}

/**
 * Every controller worth showing: attached ones, enabled packages that are
 * not plugged in, and maps whose controller is neither -- so a map is never
 * out of reach of the player who made it. Attached first, then by name.
 */
/** A controller RackForge ships from its maker's documentation: its own, and
 * with no driver. */
export function isCatalogPackage(entry: ControllerPackageSummary): boolean {
  return entry.trust === "official" && entry.runtime === "DeclarativeV1";
}

export function buildControllerDevices(
  packages: readonly ControllerPackageSummary[],
  registered: ReadonlyArray<{
    controller_id: string;
    source?: { id: string; name: string };
    connected: boolean;
    identified?: boolean;
  }>,
  maps: readonly ControllerMap[],
  /** Controllers whose map is still RackForge's factory map, as offered. */
  factoryUntouched: ReadonlySet<string> = new Set(),
): ControllerDevice[] {
  const catalog = new Set(packages.filter(isCatalogPackage).map((entry) => entry.id));
  const ids = new Set<string>([
    ...registered.map((entry) => entry.controller_id),
    // RackForge's catalog describes dozens of keyboards the player may never
    // own: one of them shows once it is plugged in or has a map of the
    // player's, not before. Every one comes with a factory map; that alone
    // does not list it.
    ...maps
      .filter((map) => !(catalog.has(map.controller_id) && factoryUntouched.has(map.controller_id)))
      .map((map) => map.controller_id),
    ...packages
      .filter((entry) => entry.enabled && !isCatalogPackage(entry))
      .map((entry) => entry.id),
  ]);
  const devices = [...ids].map((id): ControllerDevice => {
    const summary = packages.find((entry) => entry.id === id);
    const attached = registered.find((entry) => entry.controller_id === id);
    const map = maps.find((entry) => entry.controller_id === id);
    let inputs = summary?.inputs ?? [];
    let orphaned = false;
    if (inputs.length === 0 && map) {
      const seen = new Set<string>();
      inputs = map.plugins
        .flatMap((plugin) => plugin.mappings.map((mapping) => mapping.input))
        .filter((input) => (seen.has(input.id) ? false : (seen.add(input.id), true)))
        .map(inputFromMapped);
      orphaned = !summary;
    }
    return {
      id,
      name: summary?.name ?? map?.controller_name ?? attached?.source?.name ?? id,
      vendor: summary?.vendor ?? undefined,
      package: summary,
      source: attached?.source ? { id: attached.source.id, name: attached.source.name } : undefined,
      connected: attached?.connected ?? false,
      identified: attached?.identified ?? false,
      map,
      inputs,
      roles: summary?.roles ?? [],
      actions: summary?.actions ?? [],
      orphaned,
    };
  });
  return devices.sort(
    (left, right) => Number(right.connected) - Number(left.connected) || left.name.localeCompare(right.name),
  );
}

/** "synth.filter.cutoff" as the player reads it: "Filter cutoff". */
export function roleLabel(role: string): string {
  const parts = role.split(".");
  const meaningful = parts[0] === "synth" || parts[0] === "rackforge" || parts[0] === "performance" || parts[0] === "mixer" || parts[0] === "plugin"
    ? parts.slice(1)
    : parts;
  const text = meaningful
    .join(" ")
    .replace(/_/g, " ")
    // Initialisms read as themselves.
    .replace(/\b(lfo|adsr|midi|eq)\b/gi, (word) => word.toUpperCase());
  return text.charAt(0).toUpperCase() + text.slice(1);
}
