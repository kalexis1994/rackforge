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
  | { kind: "set_lane_muted"; lane: number; muted: boolean };

export interface SequencerLaneStatus {
  playing: boolean;
  queued: boolean;
  /** A stop boundary is set: still sounding, going quiet at the bar. */
  stopping?: boolean;
  muted: boolean;
  pattern_name?: string | null;
}

export interface SequencerStatus {
  running: boolean;
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
