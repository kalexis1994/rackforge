import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { Grid3X3, LogOut, Menu, Minus, Piano, Plus, ShieldAlert, X } from "lucide-react";
import { releaseVirtualMidi, sendVirtualMidi } from "./gateway";
import { hostHaptic } from "./host";
import type { SessionSnapshot } from "./types";

type ControllerMode = "keyboard" | "pads";
type KeyboardWidth = "auto" | number;
interface PadMatrix {
  columns: number;
  rows: number;
}

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const WHITE_SEMITONES = new Set([0, 2, 4, 5, 7, 9, 11]);
const EMPTY_SESSION: SessionSnapshot = {
  schema_version: 14,
  session_id: "unavailable",
  revision: 0,
  active_mode: "idle",
  master_level: 0,
  master_pan: 0,
  live: { mode: "rack" },
  instances: [],
};
const KEYBOARD_WIDTH_STORAGE_KEY = "rackforge.touch-controller.keyboard-width";
const PAD_MATRIX_STORAGE_KEY = "rackforge.touch-controller.pad-matrix";
const MIN_VISIBLE_WHITE_KEYS = 1;
const MAX_VISIBLE_WHITE_KEYS = 29;
const MAX_PAD_MATRIX_SIDE = 8;

function storedKeyboardWidth(): KeyboardWidth {
  try {
    const stored = window.localStorage.getItem(KEYBOARD_WIDTH_STORAGE_KEY);
    if (stored === "auto") return "auto";
    const numeric = Number(stored);
    if (
      Number.isInteger(numeric) &&
      numeric >= MIN_VISIBLE_WHITE_KEYS &&
      numeric <= MAX_VISIBLE_WHITE_KEYS
    ) {
      return numeric;
    }
  } catch {
    // Storage can be unavailable in hardened or ephemeral WebViews.
  }
  return "auto";
}

function storedPadMatrix(): PadMatrix {
  try {
    const match = window.localStorage.getItem(PAD_MATRIX_STORAGE_KEY)?.match(/^(\d)x(\d)$/);
    const columns = Number(match?.[1]);
    const rows = Number(match?.[2]);
    if (
      Number.isInteger(columns) && Number.isInteger(rows) &&
      columns >= 1 && rows >= 1 &&
      columns <= MAX_PAD_MATRIX_SIDE && rows <= MAX_PAD_MATRIX_SIDE
    ) {
      return { columns, rows };
    }
  } catch {
    // Storage can be unavailable in hardened or ephemeral WebViews.
  }
  return { columns: 4, rows: 4 };
}

function noteName(note: number) {
  return `${NOTE_NAMES[note % 12]}${Math.floor(note / 12) - 1}`;
}

function useCompactKeyboard() {
  const query = "(max-width: 700px) and (orientation: portrait)";
  const [compact, setCompact] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setCompact(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return compact;
}

interface KeyboardKey {
  note: number;
  black: boolean;
  left?: number;
}

function keyboardKeys(baseNote: number, whiteCount: number) {
  const keys: KeyboardKey[] = [];
  let whites = 0;
  for (let note = baseNote; whites < whiteCount; note += 1) {
    const black = !WHITE_SEMITONES.has(note % 12);
    if (black) {
      keys.push({ note, black, left: (whites / whiteCount) * 100 });
    } else {
      keys.push({ note, black: false });
      whites += 1;
    }
  }
  return keys;
}

function padNotes(baseNote: number, matrix: PadMatrix) {
  return Array.from({ length: matrix.rows * matrix.columns }, (_, index) => {
    const row = Math.floor(index / matrix.columns);
    const column = index % matrix.columns;
    return baseNote + (matrix.rows - row - 1) * matrix.columns + column;
  });
}

function maximumOctaveForSpan(span: number) {
  return Math.max(1, Math.min(7, Math.floor((127 - span) / 12) - 1));
}

export function TouchControllerPage({
  snapshot,
  connection,
  onOpenNavigation,
  onExit,
}: {
  snapshot: SessionSnapshot | null;
  connection: string;
  onOpenNavigation: () => void;
  onExit: () => void;
}) {
  const session = snapshot ?? EMPTY_SESSION;
  const [mode, setMode] = useState<ControllerMode>("keyboard");
  const [octave, setOctave] = useState(3);
  const [velocity, setVelocity] = useState(100);
  const [sustain, setSustain] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [keyboardWidth, setKeyboardWidth] = useState<KeyboardWidth>(storedKeyboardWidth);
  const [padMatrix, setPadMatrix] = useState<PadMatrix>(storedPadMatrix);
  const [activeNotes, setActiveNotes] = useState<Set<number>>(() => new Set());
  const pointerNotes = useRef(new Map<number, number>());
  const notePointers = useRef(new Map<number, Set<number>>());
  const compactKeyboard = useCompactKeyboard();
  const visibleWhiteKeys = keyboardWidth === "auto"
    ? compactKeyboard ? 8 : 15
    : keyboardWidth;
  const keyboardSpan = keyboardKeys(0, visibleWhiteKeys).at(-1)?.note ?? 0;
  const padSpan = padMatrix.rows * padMatrix.columns - 1;
  const maximumOctave = maximumOctaveForSpan(mode === "keyboard" ? keyboardSpan : padSpan);
  const baseNote = (octave + 1) * 12;
  const playable = connection === "online" && session.active_mode !== "idle";

  const activeInstance = session.instances.find(
    (instance) => instance.instance_id === session.active_instance_id,
  );
  const activeSound = activeInstance?.sounds.find(
    (sound) => sound.id === activeInstance.selected_sound_id,
  );
  const targetName = session.active_mode === "live"
    ? session.live.active_rack_id
      ? `Rack · ${session.live.active_rack_id}`
      : "Active LIVE routing"
    : activeInstance?.plugin_name ?? "No instrument selected";
  const targetDetail = session.active_mode === "live"
    ? "Notes enter the active rack"
    : activeSound?.name ?? activeInstance?.selected_sound_id ?? "Select a plugin in Play";

  const clearLocalNotes = useCallback(() => {
    pointerNotes.current.clear();
    notePointers.current.clear();
    setActiveNotes(new Set());
    setSustain(false);
  }, []);

  const panic = useCallback(() => {
    releaseVirtualMidi();
    clearLocalNotes();
  }, [clearLocalNotes]);

  useEffect(() => {
    const releaseOnBlur = () => panic();
    const releaseWhenHidden = () => {
      if (document.visibilityState !== "visible") panic();
    };
    window.addEventListener("blur", releaseOnBlur);
    document.addEventListener("visibilitychange", releaseWhenHidden);
    return () => {
      window.removeEventListener("blur", releaseOnBlur);
      document.removeEventListener("visibilitychange", releaseWhenHidden);
      panic();
    };
  }, [panic]);

  useEffect(() => {
    if (!playable) window.queueMicrotask(panic);
  }, [panic, playable]);

  useEffect(() => {
    window.queueMicrotask(panic);
  }, [panic, session.active_instance_id, session.active_mode, session.live.active_rack_id]);

  useEffect(() => {
    try {
      window.localStorage.setItem(KEYBOARD_WIDTH_STORAGE_KEY, String(keyboardWidth));
    } catch {
      // The controller still works when persistent browser storage is disabled.
    }
  }, [keyboardWidth]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        PAD_MATRIX_STORAGE_KEY,
        `${padMatrix.columns}x${padMatrix.rows}`,
      );
    } catch {
      // The controller still works when persistent browser storage is disabled.
    }
  }, [padMatrix]);

  useEffect(() => {
    if (octave > maximumOctave) {
      window.queueMicrotask(() => {
        panic();
        setOctave(maximumOctave);
      });
    }
  }, [maximumOctave, octave, panic]);

  const releasePointer = useCallback((pointerId: number) => {
    const note = pointerNotes.current.get(pointerId);
    if (note === undefined) return;
    pointerNotes.current.delete(pointerId);
    const pointers = notePointers.current.get(note);
    pointers?.delete(pointerId);
    if (pointers && pointers.size > 0) return;
    notePointers.current.delete(note);
    sendVirtualMidi(0x80, note, 0);
    setActiveNotes((current) => {
      const next = new Set(current);
      next.delete(note);
      return next;
    });
  }, []);

  const pressPointer = useCallback((pointerId: number, note: number) => {
    if (!playable || pointerNotes.current.has(pointerId)) return;
    let pointers = notePointers.current.get(note);
    if (!pointers) {
      if (!sendVirtualMidi(0x90, note, velocity)) return;
      pointers = new Set();
      notePointers.current.set(note, pointers);
      setActiveNotes((current) => new Set(current).add(note));
      hostHaptic("tap");
    }
    pointers.add(pointerId);
    pointerNotes.current.set(pointerId, note);
  }, [playable, velocity]);

  const pointerDown = (event: ReactPointerEvent<HTMLButtonElement>, note: number) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    pressPointer(event.pointerId, note);
  };

  const pointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    if (!pointerNotes.current.has(event.pointerId)) return;
    const target = document.elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>("[data-midi-note]");
    const nextNote = Number(target?.dataset.midiNote);
    if (!Number.isInteger(nextNote) || nextNote === pointerNotes.current.get(event.pointerId)) {
      return;
    }
    releasePointer(event.pointerId);
    pressPointer(event.pointerId, nextNote);
  };

  const changeMode = (next: ControllerMode) => {
    if (next === mode) return;
    panic();
    const nextSpan = next === "keyboard" ? keyboardSpan : padSpan;
    setOctave((current) => Math.min(current, maximumOctaveForSpan(nextSpan)));
    setMode(next);
  };

  const changeOctave = (amount: number) => {
    panic();
    setOctave((current) => Math.max(1, Math.min(maximumOctave, current + amount)));
  };

  const toggleSustain = () => {
    if (!playable) return;
    const next = !sustain;
    if (!sendVirtualMidi(0xb0, 64, next ? 127 : 0)) return;
    setSustain(next);
    hostHaptic("confirm");
  };

  const keys = useMemo(
    () => keyboardKeys(baseNote, visibleWhiteKeys),
    [baseNote, visibleWhiteKeys],
  );
  const pads = useMemo(() => padNotes(baseNote, padMatrix), [baseNote, padMatrix]);

  return (
    <section
      className="touch-controller"
      onPointerMove={pointerMove}
      onPointerUp={(event) => releasePointer(event.pointerId)}
      onPointerCancel={(event) => releasePointer(event.pointerId)}
      onContextMenu={(event) => event.preventDefault()}
    >
      <button
        className="touch-controller-menu-button"
        onClick={() => setMenuOpen(true)}
        aria-label="Open controller menu"
        aria-expanded={menuOpen}
      >
        <Menu aria-hidden="true" />
      </button>

      {menuOpen ? (
        <div
          className="touch-controller-menu-backdrop"
          role="presentation"
          onPointerDown={(event) => {
            if (event.target === event.currentTarget) setMenuOpen(false);
          }}
        >
          <aside className="touch-controller-menu" aria-label="Touch Controller settings">
            <header>
              <div>
                <span className="eyebrow">TOUCH CONTROLLER</span>
                <strong>{targetName}</strong>
                <small>{playable ? targetDetail : "The audio host is not ready"}</small>
              </div>
              <button onClick={() => setMenuOpen(false)} aria-label="Close controller menu">
                <X aria-hidden="true" />
              </button>
            </header>

            <div className="touch-mode-switch" aria-label="Touch controller layout">
              <button
                className={mode === "keyboard" ? "active" : ""}
                onClick={() => changeMode("keyboard")}
              >
                <Piano aria-hidden="true" />
                Keyboard
              </button>
              <button
                className={mode === "pads" ? "active" : ""}
                onClick={() => changeMode("pads")}
              >
                <Grid3X3 aria-hidden="true" />
                Pads
              </button>
            </div>

            <div className="touch-octave-control">
              <span>BASE OCTAVE</span>
              <div>
                <button onClick={() => changeOctave(-1)} disabled={octave <= 1} aria-label="Previous octave">
                  <Minus aria-hidden="true" />
                </button>
                <output>C{octave}</output>
                <button onClick={() => changeOctave(1)} disabled={octave >= maximumOctave} aria-label="Next octave">
                  <Plus aria-hidden="true" />
                </button>
              </div>
            </div>

            <label className="touch-velocity-control">
              <span>VELOCITY <output>{velocity}</output></span>
              <input
                type="range"
                min="1"
                max="127"
                value={velocity}
                onChange={(event) => setVelocity(Number(event.target.value))}
              />
            </label>

            {mode === "keyboard" ? <div className="touch-key-width-control">
              <div>
                <span>VISIBLE WHITE KEYS</span>
                <output>{keyboardWidth === "auto" ? `AUTO · ${visibleWhiteKeys}` : visibleWhiteKeys}</output>
                <button
                  className={keyboardWidth === "auto" ? "active" : ""}
                  onClick={() => {
                    panic();
                    setKeyboardWidth("auto");
                  }}
                  aria-pressed={keyboardWidth === "auto"}
                >
                  Auto
                </button>
              </div>
              <input
                type="range"
                min={MIN_VISIBLE_WHITE_KEYS}
                max={MAX_VISIBLE_WHITE_KEYS}
                step="1"
                list="touch-key-width-marks"
                value={visibleWhiteKeys}
                onChange={(event) => {
                  panic();
                  setKeyboardWidth(Number(event.target.value));
                }}
                aria-label="Visible white keys"
              />
              <datalist id="touch-key-width-marks">
                <option value="1" label="1" />
                <option value="4" label="4" />
                <option value="8" label="8" />
                <option value="11" label="11" />
                <option value="15" label="15" />
                <option value="22" label="22" />
                <option value="29" label="29" />
              </datalist>
            </div> : null}

            {mode === "pads" ? <div className="touch-pad-matrix-control">
              <div className="touch-pad-matrix-heading">
                <span>PAD MATRIX</span>
                <output>{padMatrix.columns} × {padMatrix.rows} · {padMatrix.columns * padMatrix.rows} pads</output>
              </div>
              <div
                className="touch-pad-matrix-picker"
                role="grid"
                aria-label="Choose pad matrix columns and rows"
              >
                {Array.from({ length: MAX_PAD_MATRIX_SIDE ** 2 }, (_, index) => {
                  const row = Math.floor(index / MAX_PAD_MATRIX_SIDE) + 1;
                  const column = index % MAX_PAD_MATRIX_SIDE + 1;
                  const selected = column <= padMatrix.columns && row <= padMatrix.rows;
                  return (
                    <button
                      key={`${column}x${row}`}
                      className={selected ? "selected" : ""}
                      onClick={() => {
                        if (mode !== "pads") return;
                        panic();
                        const nextMatrix = { columns: column, rows: row };
                        setOctave((current) => Math.min(
                          current,
                          maximumOctaveForSpan(nextMatrix.columns * nextMatrix.rows - 1),
                        ));
                        setPadMatrix(nextMatrix);
                      }}
                      role="gridcell"
                      aria-label={`${column} columns by ${row} rows`}
                      aria-selected={column === padMatrix.columns && row === padMatrix.rows}
                    />
                  );
                })}
              </div>
            </div> : null}

            <div className="touch-performance-actions">
              <button
                className={sustain ? "sustain active" : "sustain"}
                onClick={toggleSustain}
                disabled={!playable}
                aria-pressed={sustain}
              >
                Sustain
              </button>
              <button className="touch-panic" onClick={panic}>
                <ShieldAlert aria-hidden="true" />
                Panic
              </button>
            </div>

            <button
              className="touch-main-navigation"
              onClick={() => {
                setMenuOpen(false);
                onOpenNavigation();
              }}
            >
              <Menu aria-hidden="true" />
              RackForge menu
            </button>
            <button
              className="touch-controller-exit"
              onClick={() => {
                panic();
                setMenuOpen(false);
                onExit();
              }}
            >
              <LogOut aria-hidden="true" />
              Exit Touch Controller
            </button>
          </aside>
        </div>
      ) : null}

      <div className={`touch-instrument${playable ? "" : " disabled"}`}>
        {mode === "keyboard" ? (
          <div
            className="piano-keyboard"
            style={{ "--white-key-count": visibleWhiteKeys } as CSSProperties}
            aria-label="On-screen piano keyboard"
          >
            <div className="piano-white-keys">
              {keys.filter((key) => !key.black).map((key) => (
                <button
                  key={key.note}
                  className={`piano-key white${activeNotes.has(key.note) ? " active" : ""}`}
                  data-midi-note={key.note}
                  aria-label={noteName(key.note)}
                  disabled={!playable}
                  onPointerDown={(event) => pointerDown(event, key.note)}
                >
                  <span>{noteName(key.note)}</span>
                </button>
              ))}
            </div>
            {keys.filter((key) => key.black).map((key) => (
              <button
                key={key.note}
                className={`piano-key black${activeNotes.has(key.note) ? " active" : ""}`}
                data-midi-note={key.note}
                style={{ "--key-left": `${key.left}%` } as CSSProperties}
                aria-label={noteName(key.note)}
                disabled={!playable}
                onPointerDown={(event) => pointerDown(event, key.note)}
              >
                <span>{noteName(key.note)}</span>
              </button>
            ))}
          </div>
        ) : (
          <div
            className="touch-pad-grid"
            style={{ "--pad-columns": padMatrix.columns } as CSSProperties}
            aria-label="On-screen MIDI pads"
          >
            {pads.map((note, index) => (
              <button
                key={note}
                className={`touch-pad pad-${index + 1}${activeNotes.has(note) ? " active" : ""}`}
                data-midi-note={note}
                disabled={!playable}
                onPointerDown={(event) => pointerDown(event, note)}
                aria-label={`Pad ${index + 1}, ${noteName(note)}`}
              >
                <span>PAD {String(index + 1).padStart(2, "0")}</span>
                <strong>{noteName(note)}</strong>
                <small>MIDI {note}</small>
              </button>
            ))}
          </div>
        )}
        {!playable ? (
          <div className="touch-instrument-message">
            <ShieldAlert aria-hidden="true" />
            <strong>Controller unavailable</strong>
            <span>Connect to the RackForge audio host and select a Play or Live target.</span>
          </div>
        ) : null}
      </div>
    </section>
  );
}
