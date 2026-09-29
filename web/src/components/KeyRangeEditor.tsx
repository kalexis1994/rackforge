import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import {
  KEY_RANGE_PRESETS,
  MIDI_HIGHEST,
  MIDI_LOWEST,
  MIDDLE_C,
  PIANO_HIGHEST,
  PIANO_LOWEST,
  PIANO_WHITE_KEYS,
  type KeyRange,
  clampNote,
  describeRange,
  moveEdge,
  nearestEdge,
  noteName,
  pianoKeys,
  sameRange,
} from "../keyRange";

// The drawing, in SVG units: the 88 keys between a zone at each end for the
// MIDI notes beyond the piano, a row above for the range's edges and one
// below for the octaves' names.
const WHITE = 10;
const WHITE_HEIGHT = 66;
const BLACK_HEIGHT = 41;
const ZONE = 14;
const GAP = 3;
const PIANO_X = ZONE + GAP;
const PIANO_WIDTH = PIANO_WHITE_KEYS * WHITE;
const RIGHT_ZONE_X = PIANO_X + PIANO_WIDTH + GAP;
const WIDTH = RIGHT_ZONE_X + ZONE;
const TOP = 14;
const LABELS = 12;
const HEIGHT = TOP + WHITE_HEIGHT + LABELS;

const KEYS = pianoKeys();
const WHITE_KEYS = KEYS.filter((key) => !key.black);
const BLACK_KEYS = KEYS.filter((key) => key.black);

/** Where a note's key is centred across the drawing. */
function noteCentre(note: number): number {
  if (note < PIANO_LOWEST) return ZONE / 2;
  if (note > PIANO_HIGHEST) return RIGHT_ZONE_X + ZONE / 2;
  const key = KEYS[note - PIANO_LOWEST];
  return PIANO_X + (key.x + key.width / 2) * WHITE;
}

/** The note under a point of the drawing; the end zones stand for their notes. */
function noteAt(x: number, y: number, edge: "low" | "high" | null): number {
  if (x < PIANO_X - GAP / 2) {
    return edge === "high" ? PIANO_LOWEST - 1 : MIDI_LOWEST;
  }
  if (x > RIGHT_ZONE_X - GAP / 2) {
    return edge === "low" ? PIANO_HIGHEST + 1 : MIDI_HIGHEST;
  }
  const along = (x - PIANO_X) / WHITE;
  if (y - TOP < BLACK_HEIGHT) {
    const black = BLACK_KEYS.find((key) => along >= key.x && along <= key.x + key.width);
    if (black) return black.note;
  }
  const white = WHITE_KEYS[Math.max(0, Math.min(WHITE_KEYS.length - 1, Math.floor(along)))];
  return white.note;
}

function EdgeControl({
  label,
  note,
  minimum,
  maximum,
  onChange,
}: {
  label: string;
  note: number;
  minimum: number;
  maximum: number;
  onChange: (note: number) => void;
}) {
  const set = (next: number) => onChange(Math.max(minimum, Math.min(maximum, clampNote(next))));
  const keys = (event: KeyboardEvent<HTMLOutputElement>) => {
    const moves: Record<string, number> = {
      ArrowLeft: -1,
      ArrowDown: -1,
      ArrowRight: 1,
      ArrowUp: 1,
      PageDown: -12,
      PageUp: 12,
    };
    if (event.key === "Home") set(minimum);
    else if (event.key === "End") set(maximum);
    else if (moves[event.key] !== undefined) set(note + moves[event.key]);
    else return;
    event.preventDefault();
  };
  return (
    <div className="key-range-edge">
      <span>{label}</span>
      <div>
        <button type="button" aria-label={`${label}: an octave down`} disabled={note <= minimum} onClick={() => set(note - 12)}>«</button>
        <button type="button" aria-label={`${label}: a semitone down`} disabled={note <= minimum} onClick={() => set(note - 1)}>‹</button>
        <output
          role="slider"
          tabIndex={0}
          aria-label={label}
          aria-valuemin={minimum}
          aria-valuemax={maximum}
          aria-valuenow={note}
          aria-valuetext={`${noteName(note)}, note ${note}`}
          title="Arrows move a semitone, Page Up and Page Down an octave"
          onKeyDown={keys}
        >
          <strong>{noteName(note)}</strong>
          <small>{note}</small>
        </output>
        <button type="button" aria-label={`${label}: a semitone up`} disabled={note >= maximum} onClick={() => set(note + 1)}>›</button>
        <button type="button" aria-label={`${label}: an octave up`} disabled={note >= maximum} onClick={() => set(note + 12)}>»</button>
      </div>
    </div>
  );
}

/**
 * A MIDI connection's key range on a full piano: the keys in range are lit,
 * a touched or dragged key moves the nearer edge, and each edge can be set
 * a semitone or an octave at a time. MIDI's notes beyond the piano's 88
 * keys are the two zones at the ends.
 */
export function KeyRangeEditor({
  range,
  onChange,
}: {
  range: KeyRange;
  onChange: (range: KeyRange) => void;
}) {
  const gesture = useRef<{ pointerId: number; edge: "low" | "high"; range: KeyRange } | null>(null);

  const pointAt = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return {
      x: ((event.clientX - box.left) / box.width) * WIDTH,
      y: ((event.clientY - box.top) / box.height) * HEIGHT,
    };
  };

  const begin = (event: PointerEvent<SVGSVGElement>) => {
    if (!event.isPrimary || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const { x, y } = pointAt(event);
    const edge = nearestEdge(range, noteAt(x, y, null));
    const moved = moveEdge(range, edge, noteAt(x, y, edge));
    gesture.current = { pointerId: event.pointerId, ...moved };
    onChange(moved.range);
  };

  const drag = (event: PointerEvent<SVGSVGElement>) => {
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    event.preventDefault();
    const { x, y } = pointAt(event);
    const moved = moveEdge(current.range, current.edge, noteAt(x, y, current.edge));
    if (sameRange(moved.range, current.range)) return;
    gesture.current = { pointerId: event.pointerId, ...moved };
    onChange(moved.range);
  };

  const end = (event: PointerEvent<SVGSVGElement>) => {
    if (gesture.current?.pointerId !== event.pointerId) return;
    gesture.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const inRange = (note: number) => note >= range.low && note <= range.high;
  const lowX = noteCentre(range.low);
  const highX = noteCentre(range.high);
  const belowPiano = range.low < PIANO_LOWEST;
  const abovePiano = range.high > PIANO_HIGHEST;

  return (
    <div className="key-range">
      <div className="key-range-head">
        <strong aria-live="polite">{describeRange(range)}</strong>
        <div className="key-range-presets" role="group" aria-label="Key range presets">
          {KEY_RANGE_PRESETS.map((preset) => {
            const active = sameRange(preset.range, range);
            return (
              <button
                type="button"
                key={preset.label}
                className={active ? "active" : ""}
                aria-pressed={active}
                onClick={() => onChange(preset.range)}
              >
                {preset.label}
              </button>
            );
          })}
        </div>
      </div>

      <svg
        className="key-range-keyboard"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label={`Key range ${describeRange(range)}. Touch or drag a key to move the nearer edge.`}
        onPointerDown={begin}
        onPointerMove={drag}
        onPointerUp={end}
        onPointerCancel={end}
      >
        <title>Touch or drag a key to move the nearer edge of the range</title>
        {/* MIDI notes below A0 and above C8, lit when the range reaches them. */}
        <rect
          className={`key-range-zone${belowPiano ? " in" : ""}`}
          x={0}
          y={TOP}
          width={ZONE}
          height={WHITE_HEIGHT}
          rx={2}
        />
        <rect
          className={`key-range-zone${abovePiano ? " in" : ""}`}
          x={RIGHT_ZONE_X}
          y={TOP}
          width={ZONE}
          height={WHITE_HEIGHT}
          rx={2}
        />
        <text className="key-range-zone-mark" x={ZONE / 2} y={TOP + WHITE_HEIGHT / 2}>‹</text>
        <text className="key-range-zone-mark" x={RIGHT_ZONE_X + ZONE / 2} y={TOP + WHITE_HEIGHT / 2}>›</text>

        {WHITE_KEYS.map((key) => (
          <rect
            key={key.note}
            className={`key-range-white${inRange(key.note) ? " in" : ""}`}
            x={PIANO_X + key.x * WHITE + 0.5}
            y={TOP}
            width={WHITE - 1}
            height={WHITE_HEIGHT}
            rx={1.5}
          />
        ))}
        {BLACK_KEYS.map((key) => (
          <rect
            key={key.note}
            className={`key-range-black${inRange(key.note) ? " in" : ""}`}
            x={PIANO_X + key.x * WHITE}
            y={TOP}
            width={key.width * WHITE}
            height={BLACK_HEIGHT}
            rx={1}
          />
        ))}
        <circle className="key-range-middle-c" cx={noteCentre(MIDDLE_C)} cy={TOP + WHITE_HEIGHT - 6} r={1.6} />

        {/* The range over the keys, its edges flagged. */}
        <line className="key-range-span" x1={lowX} x2={highX} y1={TOP - 5} y2={TOP - 5} />
        <path className="key-range-flag" d={`M${lowX} ${TOP - 1} l-4 -7 h8 z`} />
        <path className="key-range-flag" d={`M${highX} ${TOP - 1} l-4 -7 h8 z`} />

        {WHITE_KEYS.filter((key) => key.note % 12 === 0).map((key) => (
          <text
            key={key.note}
            className="key-range-octave"
            x={PIANO_X + (key.x + 0.5) * WHITE}
            y={HEIGHT - 2}
          >
            {noteName(key.note)}
          </text>
        ))}
      </svg>

      <div className="key-range-edges">
        <EdgeControl
          label="Lowest key"
          note={range.low}
          minimum={MIDI_LOWEST}
          maximum={range.high}
          onChange={(low) => onChange({ low, high: range.high })}
        />
        <EdgeControl
          label="Highest key"
          note={range.high}
          minimum={range.low}
          maximum={MIDI_HIGHEST}
          onChange={(high) => onChange({ low: range.low, high })}
        />
      </div>
    </div>
  );
}
