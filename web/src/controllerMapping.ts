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
  MidiActivityEvent,
  ParameterLinkMessage,
  ParameterLinkMode,
  PluginParameterDescriptor,
  PluginParameterSnapshot,
} from "./types";

export type InputKind = "knob" | "fader" | "encoder" | "button" | "pad" | "wheel" | "pedal";

/** One physical control, as a controller package declares it. */
export interface ControllerInput {
  id: string;
  name: string;
  kind: InputKind;
  group?: string;
  /** Zero-based channel, and exactly one of the three messages. */
  midi: { channel: number; cc?: number; note?: number; pitch_bend?: boolean };
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

/** The input as a mapping records it: its id, its name, and a copy of its message. */
export function mappedInputFor(input: ControllerInput): ControlMapping["input"] | null {
  const message = inputMessage(input);
  if (!message) return null;
  return {
    id: input.id,
    name: input.name,
    // Links name channels from 1, packages from 0.
    channel: { mode: "channel", channel: input.midi.channel + 1 },
    message,
  };
}

/** Which input, if any, a received message comes from. */
export function inputForActivity(
  event: Pick<MidiActivityEvent, "status" | "data1">,
  inputs: readonly ControllerInput[],
): ControllerInput | undefined {
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
  const channel = `ch ${input.midi.channel + 1}`;
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
  input: Pick<ControllerInput, "kind">,
  parameter: PluginParameterDescriptor,
): ModeKind[] {
  const kind = parameter.kind.type;
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

/**
 * The map with this mapping in place of whatever the same input -- by id or
 * by message -- did in the same plugin: an input does one thing in each
 * plugin. The map is not changed; a new one is returned.
 */
export function withMapping(
  map: ControllerMap,
  plugin: { plugin_id: string; plugin_name: string },
  mapping: ControlMapping,
): ControllerMap {
  const others = map.plugins.filter((entry) => entry.plugin_id !== plugin.plugin_id);
  const existing = map.plugins.find((entry) => entry.plugin_id === plugin.plugin_id);
  const kept = (existing?.mappings ?? []).filter(
    (candidate) =>
      candidate.id !== mapping.id
      && candidate.input.id !== mapping.input.id
      && !sameMessage(candidate.input, mapping.input),
  );
  return {
    ...map,
    plugins: [...others, { plugin_id: plugin.plugin_id, plugin_name: plugin.plugin_name, mappings: [...kept, mapping] }],
  };
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
  map?: ControllerMap;
  inputs: ControllerInput[];
  roles: ControllerRole[];
  actions: ControllerAction[];
  /** Inputs known only from the map: the package that named them is gone. */
  orphaned: boolean;
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
export function buildControllerDevices(
  packages: readonly ControllerPackageSummary[],
  registered: ReadonlyArray<{ controller_id: string; source?: { id: string; name: string }; connected: boolean }>,
  maps: readonly ControllerMap[],
): ControllerDevice[] {
  const ids = new Set<string>([
    ...registered.map((entry) => entry.controller_id),
    ...maps.map((map) => map.controller_id),
    ...packages.filter((entry) => entry.enabled).map((entry) => entry.id),
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
