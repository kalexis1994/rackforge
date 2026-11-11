import {
  type ControllerDevice,
  type ControllerInput,
  groupInputs,
  inputForActivity,
  inputMessageLabel,
} from "./controllerMapping";
import type { MidiActivityEvent } from "./types";

/**
 * Checking a controller against its package: the player moves every control,
 * and each message the host hears is set against what the package declares.
 * A control ticks off when its message arrives; a message no control declares
 * is kept as a stray, with the control it is probably meant to be. The report
 * is what a correction to the package's SOURCES.md is written from.
 */

/** What one control, or one stray message, was heard sending. */
export interface HeardValues {
  count: number;
  /** The values: a control change's, a note-on's velocity, a bend's 14 bits. */
  min: number;
  max: number;
  /** Distinct values, in order, up to {@link MAX_DISTINCT}. */
  values: number[];
  /** A note that also ended: a note-off, or a note-on at velocity 0. */
  released?: boolean;
}

export interface StrayMessage extends HeardValues {
  label: string;
  /** The declared control this message most resembles, as the player reads it. */
  near?: string;
}

export interface ControllerCheck {
  deviceId: string;
  startedAt: string;
  heard: Record<string, HeardValues>;
  strays: Record<string, StrayMessage>;
}

/** Enough to tell a button's two values from a knob's travel. */
const MAX_DISTINCT = 128;

export function startCheck(deviceId: string, now: Date = new Date()): ControllerCheck {
  return { deviceId, startedAt: now.toISOString(), heard: {}, strays: {} };
}

type CheckEvent = Pick<MidiActivityEvent, "status" | "data1" | "data2">;

function heardValue(event: CheckEvent): { value?: number; released?: boolean } {
  const kind = event.status & 0xf0;
  if (event.status >= 0xf0) return {};
  if (kind === 0x80) return { released: true };
  if (kind === 0x90) return event.data2 === 0 ? { released: true } : { value: event.data2 };
  if (kind === 0xe0) return { value: event.data1 | (event.data2 << 7) };
  if (kind === 0xc0 || kind === 0xd0) return { value: event.data1 };
  return { value: event.data2 };
}

function withHeard<T extends HeardValues>(previous: T | undefined, event: CheckEvent, fresh: () => T): T {
  const { value, released } = heardValue(event);
  const base = previous ?? fresh();
  const next: T = { ...base, count: base.count + 1 };
  if (released) next.released = true;
  if (value !== undefined) {
    const first = base.values.length === 0 && base.min > base.max;
    next.min = first ? value : Math.min(base.min, value);
    next.max = first ? value : Math.max(base.max, value);
    if (!base.values.includes(value) && base.values.length < MAX_DISTINCT) {
      next.values = [...base.values, value].sort((left, right) => left - right);
    }
  }
  return next;
}

const empty = (): HeardValues => ({ count: 0, min: 1, max: 0, values: [] });

const CHANNEL_KINDS: Record<number, string> = {
  0x80: "Note",
  0x90: "Note",
  0xa0: "Poly pressure",
  0xb0: "CC",
  0xc0: "Program change",
  0xd0: "Channel pressure",
  0xe0: "Pitch bend",
};

/** A message as the player checks it, without its value: "CC 21 · ch 16". */
export function activityLabel(event: CheckEvent): string {
  if (event.status === 0xfa) return "MIDI Start";
  if (event.status === 0xfb) return "MIDI Continue";
  if (event.status === 0xfc) return "MIDI Stop";
  if (event.status >= 0xf0) return `System message ${event.status.toString(16).toUpperCase()}h`;
  const kind = event.status & 0xf0;
  const channel = `ch ${(event.status & 0x0f) + 1}`;
  const name = CHANNEL_KINDS[kind];
  if (kind === 0xc0 || kind === 0xd0 || kind === 0xe0) return `${name} · ${channel}`;
  return `${name} ${event.data1} · ${channel}`;
}

/**
 * The declared control a stray is probably meant to be: the same number on
 * another channel, or the same number sent as a note instead of a control
 * change (or the other way round).
 */
function nearestInput(event: CheckEvent, inputs: readonly ControllerInput[]): ControllerInput | undefined {
  const kind = event.status & 0xf0;
  if (event.status >= 0xf0 || kind === 0xc0 || kind === 0xd0) return undefined;
  const channel = event.status & 0x0f;
  const note = kind === 0x80 || kind === 0x90 || kind === 0xa0;
  const sameNumber = (input: ControllerInput) =>
    note ? input.midi.note === event.data1 : kind === 0xb0 ? input.midi.cc === event.data1 : input.midi.pitch_bend === true;
  const otherKind = (input: ControllerInput) =>
    note ? input.midi.cc === event.data1 : kind === 0xb0 && input.midi.note === event.data1;
  return (
    inputs.find((input) => sameNumber(input) && input.midi.channel !== channel)
    ?? inputs.find((input) => otherKind(input) && input.midi.channel === channel)
    ?? inputs.find(otherKind)
  );
}

/** Folds the messages heard from the controller's port into the check. */
export function recordCheckEvents(
  check: ControllerCheck,
  events: readonly CheckEvent[],
  inputs: readonly ControllerInput[],
): ControllerCheck {
  if (events.length === 0) return check;
  const heard = { ...check.heard };
  const strays = { ...check.strays };
  for (const event of events) {
    // A pad's pressure is its own message, not the pad: it is a stray unless
    // a control declares it, and inputForActivity knows no control that does.
    const input = inputForActivity(event, inputs);
    if (input) {
      heard[input.id] = withHeard(heard[input.id], event, empty);
      continue;
    }
    const label = activityLabel(event);
    strays[label] = withHeard(strays[label], event, () => {
      const near = nearestInput(event, inputs);
      return { ...empty(), label, ...(near ? { near: `${near.name}: ${inputMessageLabel(near)}` } : {}) };
    });
  }
  return { ...check, heard, strays };
}

/** "0, 127", or "0–127 (97 values)" for a travel. */
export function valuesLabel(values: HeardValues, note: boolean): string {
  const parts: string[] = [];
  if (values.values.length > 0) {
    parts.push(
      values.values.length <= 6
        ? values.values.join(", ")
        : `${values.min}–${values.max} (${values.values.length}${values.values.length === MAX_DISTINCT ? "+" : ""} values)`,
    );
  }
  if (note) parts.push(values.released ? "released" : "no release heard");
  return parts.join(" · ") || "—";
}

/**
 * What the values say that the package does not: an endless encoder
 * declared absolute that sends only small steps either side of a centre is
 * relative. Only a hint: it takes a few turns each way.
 */
export function checkHint(input: ControllerInput, values: HeardValues): string | null {
  if (typeof input.midi.cc !== "number" || values.values.length < 2 || values.count < 6) return null;
  const relative = Boolean(input.encoder?.startsWith("relative"));
  const both = (low: [number, number], high: [number, number]) =>
    values.values.some((value) => value >= low[0] && value <= low[1])
    && values.values.some((value) => value >= high[0] && value <= high[1])
    && values.values.every((value) => (value >= low[0] && value <= low[1]) || (value >= high[0] && value <= high[1]));
  if (!relative && (input.kind === "knob" || input.kind === "encoder")) {
    if (both([56, 63], [65, 72])) return "Looks relative (binary offset, 64 is still)";
    if (both([1, 8], [120, 127])) return "Looks relative (two's complement)";
    if (both([1, 8], [65, 72])) return "Looks relative (sign and magnitude)";
  }
  // An absolute knob swept slowly passes through both quarters a relative
  // encoder only reaches when spun hard.
  if (relative && values.values.length > 24
    && values.values.some((value) => value >= 20 && value <= 44)
    && values.values.some((value) => value >= 84 && value <= 108)) {
    return "Looks absolute: it sweeps a range";
  }
  return null;
}

export function checkProgress(check: ControllerCheck, inputs: readonly ControllerInput[]): { heard: number; total: number } {
  return { heard: inputs.filter((input) => check.heard[input.id]).length, total: inputs.length };
}

/** "1 message", "3 messages". */
export function strayCount(check: ControllerCheck): string {
  const count = Object.keys(check.strays).length;
  return `${count} ${count === 1 ? "message" : "messages"}`;
}

const cell = (text: string) => text.replace(/\|/g, "\\|");

/** The check as Markdown, to paste into an issue or a package's SOURCES.md. */
export function checkReport(device: ControllerDevice, check: ControllerCheck, now: Date = new Date()): string {
  const { heard, total } = checkProgress(check, device.inputs);
  const summary = device.package;
  const lines = [
    `# Hardware check: ${device.name}`,
    "",
    summary
      ? `- Package: \`${summary.id}\`${summary.version ? ` ${summary.version}` : ""}`
      : `- Package: none listed for \`${device.id}\`; its controls are the map's`,
    `- Port RackForge reads: ${device.source?.name ?? "not connected"}`,
    `- Recognised by its Identity Reply: ${device.identified ? "yes" : "no (port name only)"}`,
    `- Checked: ${check.startedAt} to ${now.toISOString()}`,
    "- Firmware: (fill in)",
    `- Heard ${heard} of ${total} controls; ${strayCount(check)} no control declares`,
    "",
    "Only the port RackForge reads is heard: a control that sends on another port of the device shows as not heard.",
    "",
    "## Controls",
    "",
    "| Group | Control | Declares | Heard | Values | Note |",
    "|---|---|---|---|---|---|",
  ];
  for (const group of groupInputs(device.inputs)) {
    for (const input of group.inputs) {
      const values = check.heard[input.id];
      const hint = values ? checkHint(input, values) : null;
      lines.push(
        `| ${cell(group.group)} | ${cell(input.name)} | ${cell(inputMessageLabel(input))} | ${values ? values.count : "no"} | ${values ? cell(valuesLabel(values, typeof input.midi.note === "number")) : ""} | ${hint ? cell(hint) : ""} |`,
      );
    }
  }
  const strays = Object.values(check.strays).sort((left, right) => right.count - left.count);
  lines.push("", "## Messages no control declares", "");
  if (strays.length === 0) {
    lines.push("None.");
  } else {
    lines.push("| Message | Times | Values | Close to |", "|---|---|---|---|");
    for (const stray of strays) {
      lines.push(`| ${cell(stray.label)} | ${stray.count} | ${cell(valuesLabel(stray, stray.label.startsWith("Note ")))} | ${cell(stray.near ?? "")} |`);
    }
  }
  return `${lines.join("\n")}\n`;
}
