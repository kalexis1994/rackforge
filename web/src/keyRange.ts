/**
 * A MIDI connection's key range, drawn on the 88 keys of a piano: which keys
 * are white or black and where each one sits, which edge of the range a
 * touched key moves, and the ranges a player reaches for.
 *
 * MIDI notes run 0-127; a piano's keys A0-C8 are 21-108. The notes beyond
 * the piano stay reachable: the keyboard shows them as one zone at each end.
 */

export const MIDI_LOWEST = 0;
export const MIDI_HIGHEST = 127;
/** A0 and C8, the piano's lowest and highest keys. */
export const PIANO_LOWEST = 21;
export const PIANO_HIGHEST = 108;
export const MIDDLE_C = 60;

export interface KeyRange {
  low: number;
  high: number;
}

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** Scientific pitch name, middle C (60) being C4. */
export function noteName(note: number): string {
  return `${NAMES[((note % 12) + 12) % 12]}${Math.floor(note / 12) - 1}`;
}

export function isBlackKey(note: number): boolean {
  return [1, 3, 6, 8, 10].includes(((note % 12) + 12) % 12);
}

export function clampNote(note: number): number {
  return Math.max(MIDI_LOWEST, Math.min(MIDI_HIGHEST, Math.round(note)));
}

export interface PianoKey {
  note: number;
  black: boolean;
  /** Left edge and width, in white-key widths from A0's left edge. */
  x: number;
  width: number;
}

/** How wide a black key is, and how far it reaches into the white keys. */
export const BLACK_KEY_WIDTH = 0.6;

/** The piano's 88 keys, white and black, laid out left to right. */
export function pianoKeys(): PianoKey[] {
  const keys: PianoKey[] = [];
  let whites = 0;
  for (let note = PIANO_LOWEST; note <= PIANO_HIGHEST; note += 1) {
    if (isBlackKey(note)) {
      keys.push({ note, black: true, x: whites - BLACK_KEY_WIDTH / 2, width: BLACK_KEY_WIDTH });
    } else {
      keys.push({ note, black: false, x: whites, width: 1 });
      whites += 1;
    }
  }
  return keys;
}

/** A0-C8 has 52 white keys. */
export const PIANO_WHITE_KEYS = 52;

/** Which edge a touched note moves: the one it lies beyond, or else the nearer. */
export function nearestEdge(range: KeyRange, note: number): "low" | "high" {
  if (note <= range.low) return "low";
  if (note >= range.high) return "high";
  return note - range.low <= range.high - note ? "low" : "high";
}

/**
 * Moves one edge to a note. An edge dragged past the other takes its place,
 * so a range is never inverted: the edge being moved is returned with it.
 */
export function moveEdge(
  range: KeyRange,
  edge: "low" | "high",
  note: number,
): { range: KeyRange; edge: "low" | "high" } {
  const target = clampNote(note);
  if (edge === "low") {
    return target <= range.high
      ? { range: { low: target, high: range.high }, edge: "low" }
      : { range: { low: range.high, high: target }, edge: "high" };
  }
  return target >= range.low
    ? { range: { low: range.low, high: target }, edge: "high" }
    : { range: { low: target, high: range.low }, edge: "low" };
}

export interface KeyRangePreset {
  label: string;
  range: KeyRange;
}

/** The ranges a player reaches for: everything, a piano, and a split at C4. */
export const KEY_RANGE_PRESETS: KeyRangePreset[] = [
  { label: "All notes", range: { low: MIDI_LOWEST, high: MIDI_HIGHEST } },
  { label: "88 keys", range: { low: PIANO_LOWEST, high: PIANO_HIGHEST } },
  { label: "Below C4", range: { low: MIDI_LOWEST, high: MIDDLE_C - 1 } },
  { label: "C4 and up", range: { low: MIDDLE_C, high: MIDI_HIGHEST } },
];

export function sameRange(left: KeyRange, right: KeyRange): boolean {
  return left.low === right.low && left.high === right.high;
}

/** "C2–G5, 44 keys", or one key's name. */
export function describeRange(range: KeyRange): string {
  const count = range.high - range.low + 1;
  if (count === 1) return `${noteName(range.low)} only`;
  return `${noteName(range.low)}–${noteName(range.high)}, ${count} keys`;
}

/**
 * What the instrument receives once the range is transposed: the notes that
 * leave MIDI's 0-127 are dropped, so the range is cut to what remains.
 */
export function transposedRange(range: KeyRange, transpose: number): KeyRange | null {
  const low = Math.max(MIDI_LOWEST, range.low + transpose);
  const high = Math.min(MIDI_HIGHEST, range.high + transpose);
  return low <= high ? { low, high } : null;
}
