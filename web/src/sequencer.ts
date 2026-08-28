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

export function emptyPattern(name: string, bars: number, beatsPerBar: number): PatternDefinition {
  return {
    id: scopedId("pattern"),
    name,
    length_ticks: Math.max(1, bars) * beatsPerBar * TICKS_PER_BEAT,
    notes: [],
  };
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
