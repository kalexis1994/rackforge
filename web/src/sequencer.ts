/**
 * The sequencer surface's model: pattern grid arithmetic, wire shapes and
 * the tap-tempo fold, kept out of the component so they can be tested flat.
 *
 * Everything here mirrors the host contract. Ticks are the only time unit a
 * document knows (960 per beat, same constant as the library schema), and
 * the wire shapes match `rackforge-control-api` field for field.
 */

import { scopedId } from "./ids";
import type { PatternDefinition, PatternNoteSpec } from "./types";

export const TICKS_PER_BEAT = 960;
/** The grid edits in 16ths: fine enough to feel, coarse enough to read. */
export const STEP_TICKS = TICKS_PER_BEAT / 4;
export const MAX_SEQUENCER_LANES = 8;

export type SequencerQuantize = "now" | "next_beat" | "next_bar";

export type SequencerScale =
  | "chromatic"
  | "major"
  | "minor"
  | "dorian"
  | "mixolydian"
  | "pentatonic_major"
  | "pentatonic_minor";

export const SCALES: ReadonlyArray<{ id: SequencerScale; label: string }> = [
  { id: "chromatic", label: "CHROMATIC" },
  { id: "major", label: "MAJOR" },
  { id: "minor", label: "MINOR" },
  { id: "dorian", label: "DORIAN" },
  { id: "mixolydian", label: "MIXOLYDIAN" },
  { id: "pentatonic_major", label: "PENT MAJ" },
  { id: "pentatonic_minor", label: "PENT MIN" },
];

export type SequencerCommand =
  | { kind: "transport_start" }
  | { kind: "transport_stop" }
  | { kind: "transport_panic" }
  | { kind: "set_tempo"; bpm: number }
  | { kind: "set_signature"; beats_per_bar: number; beat_unit: number }
  | {
      kind: "queue_pattern";
      lane: number;
      pattern: PatternDefinition;
      quantize: SequencerQuantize;
    }
  | { kind: "launch_lane"; lane: number; quantize: SequencerQuantize }
  | { kind: "stop_lane"; lane: number; quantize: SequencerQuantize }
  | { kind: "set_lane_muted"; lane: number; muted: boolean }
  | { kind: "set_lane_follow"; lane: number; scale?: SequencerScale }
  | { kind: "set_fill"; on: boolean }
  | { kind: "set_clock_out"; on: boolean }
  | { kind: "set_capture"; lane: number; on: boolean }
  | { kind: "load_slot"; lane: number; slot: number; pattern: PatternDefinition }
  | { kind: "launch_slot"; lane: number; slot: number; quantize: SequencerQuantize };

export const LANE_SLOTS = 4;
export const SLOT_LABELS = ["A", "B", "C", "D"] as const;

export interface SequencerLaneStatus {
  /** Which variation slot is the lane's active one. */
  active_slot?: number;
  /** The names stored per slot; null is an empty slot. */
  slots?: (string | null)[];
  playing: boolean;
  queued: boolean;
  /** A stop boundary is set: still sounding, going quiet at the bar. */
  stopping?: boolean;
  /** The lane is in key-follow, waiting on (or following) a held key. */
  following?: boolean;
  /** The lane is armed for live capture. */
  capturing?: boolean;
  muted: boolean;
  pattern_name?: string | null;
}

export interface SequencerStatus {
  running: boolean;
  /** The FILL performance switch is held. */
  fill?: boolean;
  /** MIDI clock out is running. */
  clock_out?: boolean;
  tempo_bpm: number;
  beats_per_bar: number;
  beat_unit: number;
  bar: number;
  beat_in_bar: number;
  beat_phase: number;
  lanes: SequencerLaneStatus[];
}

/** The instrument rows the step grid shows, top row first. General MIDI
 * drum keys: the one vocabulary every drum instrument answers to. */
export const STEP_ROWS: ReadonlyArray<{ key: number; label: string }> = [
  { key: 51, label: "RIDE" },
  { key: 49, label: "CRASH" },
  { key: 46, label: "OP HAT" },
  { key: 42, label: "CL HAT" },
  { key: 45, label: "TOM HI" },
  { key: 41, label: "TOM LO" },
  { key: 38, label: "SNARE" },
  { key: 36, label: "KICK" },
];

export function emptyPattern(
  name: string,
  bars: number,
  beatsPerBar: number,
  view: "drum" | "melodic" = "drum",
): PatternDefinition {
  return {
    id: scopedId("pattern"),
    name,
    length_ticks: Math.max(1, bars) * beatsPerBar * TICKS_PER_BEAT,
    notes: [],
    view,
    swing_percent: SWING_STRAIGHT,
    root_key: 48,
  };
}

export const SWING_STRAIGHT = 50;
export const SWING_MAX = 75;

/** Sets the pattern's groove, clamped to the MPC range. */
export function setSwing(pattern: PatternDefinition, percent: number): PatternDefinition {
  return {
    ...pattern,
    swing_percent: Math.min(SWING_MAX, Math.max(SWING_STRAIGHT, Math.round(percent))),
  };
}

/* ------------------------------------------------- the melodic lens ---
   One voice, one step lane: the grammar of the classic analog sequencers.
   Each step holds at most one note; ties stretch a note across following
   steps; velocity walks three musical levels. All of it is plain notes in
   the same document the drum lens edits. */

export const MELODIC_VELOCITIES = [64, 100, 127] as const;

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** "C3" for MIDI 60, the convention synth panels print. */
export function noteName(key: number): string {
  return `${NOTE_NAMES[key % 12]}${Math.floor(key / 12) - 2}`;
}

export function melodicStepNote(
  pattern: PatternDefinition,
  step: number,
): PatternNoteSpec | undefined {
  const tick = step * STEP_TICKS;
  return pattern.notes.find((note) => note.tick === tick);
}

/** Sets the single note of a step, replacing whatever the step held. */
export function setMelodicStep(
  pattern: PatternDefinition,
  step: number,
  key: number,
  velocity = 100,
  durationTicks = STEP_TICKS,
): PatternDefinition {
  const tick = step * STEP_TICKS;
  const clamped = Math.min(127, Math.max(0, Math.round(key)));
  const notes = pattern.notes.filter((note) => note.tick !== tick);
  notes.push({ tick, duration_ticks: durationTicks, key: clamped, velocity, channel: 0 });
  notes.sort((a, b) => a.tick - b.tick);
  return { ...pattern, notes };
}

export function clearMelodicStep(pattern: PatternDefinition, step: number): PatternDefinition {
  const tick = step * STEP_TICKS;
  return { ...pattern, notes: pattern.notes.filter((note) => note.tick !== tick) };
}

/** Moves a step's pitch by semitones, clamped to the MIDI range. */
export function transposeMelodicStep(
  pattern: PatternDefinition,
  step: number,
  semitones: number,
): PatternDefinition {
  const note = melodicStepNote(pattern, step);
  if (!note) return pattern;
  return setMelodicStep(pattern, step, note.key + semitones, note.velocity, note.duration_ticks);
}

/** Cycles a step's length through 1, 2 and 4 steps — the tie ladder. */
export function cycleMelodicTie(pattern: PatternDefinition, step: number): PatternDefinition {
  const note = melodicStepNote(pattern, step);
  if (!note) return pattern;
  const steps = Math.round(note.duration_ticks / STEP_TICKS);
  const next = steps >= 4 ? 1 : steps * 2;
  return setMelodicStep(pattern, step, note.key, note.velocity, next * STEP_TICKS);
}

/** Walks soft, medium, accent. */
export function cycleMelodicVelocity(pattern: PatternDefinition, step: number): PatternDefinition {
  const note = melodicStepNote(pattern, step);
  if (!note) return pattern;
  const index = MELODIC_VELOCITIES.indexOf(
    note.velocity as (typeof MELODIC_VELOCITIES)[number],
  );
  const next = MELODIC_VELOCITIES[(index + 1) % MELODIC_VELOCITIES.length] ?? 100;
  return setMelodicStep(pattern, step, note.key, next, note.duration_ticks);
}

export function stepCount(pattern: PatternDefinition): number {
  return Math.max(1, Math.round(pattern.length_ticks / STEP_TICKS));
}

export function hasStep(pattern: PatternDefinition, key: number, step: number): boolean {
  const tick = step * STEP_TICKS;
  return pattern.notes.some((note) => note.key === key && note.tick === tick);
}

/** Toggles one cell, returning a new document. A placed note lasts one grid
 * step; the engine clamps anything that would spill past the pattern end. */
export function toggleStep(
  pattern: PatternDefinition,
  key: number,
  step: number,
  velocity = 100,
): PatternDefinition {
  const tick = step * STEP_TICKS;
  const existing = pattern.notes.findIndex((note) => note.key === key && note.tick === tick);
  const notes =
    existing >= 0
      ? pattern.notes.filter((_, index) => index !== existing)
      : [
          ...pattern.notes,
          { tick, duration_ticks: STEP_TICKS, key, velocity, channel: 0 } satisfies PatternNoteSpec,
        ];
  return { ...pattern, notes };
}

/* --------------------------------------------------- the trig grammar ---
   Per-step chance and condition, walked as ladders the way a panel key
   cycles a setting. Both live on the note; the engine rolls a seeded die
   so the same pattern is the same show every night. */

import type { TrigCondition } from "./types";

export const PROBABILITY_LADDER = [100, 75, 50, 25] as const;

export const CONDITION_LADDER: ReadonlyArray<TrigCondition> = [
  "always",
  { cycle: { hit: 1, of: 2 } },
  { cycle: { hit: 2, of: 2 } },
  { cycle: { hit: 1, of: 4 } },
  { cycle: { hit: 3, of: 4 } },
  "fill",
  "not_fill",
  "pre",
  "not_pre",
];

export function conditionLabel(condition: TrigCondition | undefined): string {
  if (!condition || condition === "always") return "ALWAYS";
  if (typeof condition === "object") return `${condition.cycle.hit}:${condition.cycle.of}`;
  switch (condition) {
    case "fill":
      return "FILL";
    case "not_fill":
      return "NOT FILL";
    case "pre":
      return "PRE";
    case "not_pre":
      return "NOT PRE";
    default:
      return "ALWAYS";
  }
}

function sameCondition(a: TrigCondition | undefined, b: TrigCondition): boolean {
  const left = a ?? "always";
  if (typeof left === "object" && typeof b === "object") {
    return left.cycle.hit === b.cycle.hit && left.cycle.of === b.cycle.of;
  }
  return left === b;
}

function editNoteAt(
  pattern: PatternDefinition,
  tick: number,
  key: number | null,
  edit: (note: PatternNoteSpec) => PatternNoteSpec,
): PatternDefinition {
  return {
    ...pattern,
    notes: pattern.notes.map((note) =>
      note.tick === tick && (key === null || note.key === key) ? edit(note) : note,
    ),
  };
}

/** Walks one step's chance down the ladder: 100 → 75 → 50 → 25 → 100. */
export function cycleProbability(
  pattern: PatternDefinition,
  step: number,
  key: number | null = null,
): PatternDefinition {
  return editNoteAt(pattern, step * STEP_TICKS, key, (note) => {
    const index = PROBABILITY_LADDER.indexOf(
      (note.probability ?? 100) as (typeof PROBABILITY_LADDER)[number],
    );
    return {
      ...note,
      probability: PROBABILITY_LADDER[(index + 1) % PROBABILITY_LADDER.length] ?? 100,
    };
  });
}

/** Walks one step's condition through the Elektron ladder. */
export function cycleCondition(
  pattern: PatternDefinition,
  step: number,
  key: number | null = null,
): PatternDefinition {
  return editNoteAt(pattern, step * STEP_TICKS, key, (note) => {
    const index = CONDITION_LADDER.findIndex((candidate) =>
      sameCondition(note.condition, candidate),
    );
    return {
      ...note,
      condition: CONDITION_LADDER[(index + 1) % CONDITION_LADDER.length],
    };
  });
}

/** The lock a step carries, if any. One editable lock per step in this
 * surface; the engine honours up to four. */
export function stepLock(
  pattern: PatternDefinition,
  step: number,
  key: number | null = null,
) {
  const tick = step * STEP_TICKS;
  const note = pattern.notes.find(
    (candidate) => candidate.tick === tick && (key === null || candidate.key === key),
  );
  return note?.locks?.[0];
}

/** Freezes one knob into a step, replacing the step's existing lock. */
export function setStepLock(
  pattern: PatternDefinition,
  step: number,
  key: number | null,
  parameter: number,
  value: number,
): PatternDefinition {
  const tick = step * STEP_TICKS;
  return {
    ...pattern,
    notes: pattern.notes.map((note) =>
      note.tick === tick && (key === null || note.key === key)
        ? { ...note, locks: [{ parameter, value }] }
        : note,
    ),
  };
}

export function clearStepLock(
  pattern: PatternDefinition,
  step: number,
  key: number | null = null,
): PatternDefinition {
  const tick = step * STEP_TICKS;
  return {
    ...pattern,
    notes: pattern.notes.map((note) =>
      note.tick === tick && (key === null || note.key === key)
        ? { ...note, locks: [] }
        : note,
    ),
  };
}

/* --------------------------------------------------- live capture ---
   The engine records what the player did in absolute transport beats;
   the surface owns the grid, so quantising and merging happen here. */

export interface CapturedNote {
  beat: number;
  key: number;
  velocity: number;
  duration_beats: number;
}

/**
 * Merges a capture take into a pattern: onsets snap to the nearest 16th,
 * wrap around the loop, durations round up to at least one step, and a
 * captured note replaces whatever its step held — the last performance
 * wins, which is how overdubbing a groovebox feels.
 */
export function mergeCapturedNotes(
  pattern: PatternDefinition,
  captured: CapturedNote[],
): PatternDefinition {
  if (captured.length === 0) return pattern;
  const lengthSteps = stepCount(pattern);
  const melodic = pattern.view === "melodic";
  let notes = [...pattern.notes];
  for (const take of captured) {
    const step =
      ((Math.round((take.beat * TICKS_PER_BEAT) / STEP_TICKS) % lengthSteps) + lengthSteps) %
      lengthSteps;
    const tick = step * STEP_TICKS;
    const durationSteps = Math.max(
      1,
      Math.min(lengthSteps, Math.round((take.duration_beats * TICKS_PER_BEAT) / STEP_TICKS)),
    );
    notes = notes.filter(
      (note) => !(note.tick === tick && (melodic || note.key === take.key)),
    );
    notes.push({
      tick,
      duration_ticks: durationSteps * STEP_TICKS,
      key: take.key,
      velocity: Math.min(127, Math.max(1, take.velocity)),
      channel: 0,
    });
  }
  notes.sort((a, b) => a.tick - b.tick || a.key - b.key);
  return { ...pattern, notes };
}

/**
 * Folds tap timestamps (seconds) into a tempo, mirroring the host's
 * `tap_tempo` exactly: the last five taps, and a long gap or a sudden
 * halving/doubling starts a fresh session so a pause between attempts never
 * pollutes the answer.
 */
export function tapTempo(tapsSeconds: number[]): number | null {
  if (tapsSeconds.length < 2) return null;
  const recent = tapsSeconds.slice(-5);
  const intervals: number[] = [];
  for (let index = 1; index < recent.length; index += 1) {
    const interval = recent[index] - recent[index - 1];
    if (interval <= 0) return null;
    intervals.push(interval);
  }
  let start = 0;
  for (let index = 1; index < intervals.length; index += 1) {
    const previous = intervals[index - 1];
    const current = intervals[index];
    if (current > 2.0 || current > previous * 2.0 || current < previous / 2.0) {
      start = index;
    }
  }
  const used = intervals.slice(start);
  if (used.length === 0) return null;
  const mean = used.reduce((sum, value) => sum + value, 0) / used.length;
  const bpm = 60.0 / mean;
  return Math.min(400, Math.max(20, bpm));
}
