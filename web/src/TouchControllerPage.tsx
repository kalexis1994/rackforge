import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
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

interface KeyboardViewport {
  start: number;
  width: number;
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
const KEYBOARD_VIEWPORT_STORAGE_KEY = "rackforge.touch-controller.keyboard-viewport.v1";
const PAD_MATRIX_STORAGE_KEY = "rackforge.touch-controller.pad-matrix";
const DOCK_HEIGHT_STORAGE_KEY = "rackforge.touch-controller.dock-heights.v1";
const MIN_VISIBLE_WHITE_KEYS = 1;
const MAX_VISIBLE_WHITE_KEYS = 29;
const MAX_PAD_MATRIX_SIDE = 8;
const FULL_KEYBOARD_START_NOTE = 21;
const FULL_KEYBOARD_WHITE_KEYS = 52;
const FULL_KEYBOARD_MIN_WHITE_KEY_PX = 22;
const WINDOWED_KEY_MIN_PX = 42;
const PAD_ASPECT_RATIO = 1;

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}

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

function storedKeyboardViewport(): KeyboardViewport | null {
  try {
    const stored = JSON.parse(
      window.localStorage.getItem(KEYBOARD_VIEWPORT_STORAGE_KEY) ?? "null",
    ) as KeyboardViewport | null;
    if (
      Number.isFinite(stored?.start) &&
      Number.isFinite(stored?.width) &&
      stored!.start >= 0 &&
      stored!.width > 0 &&
      stored!.start + stored!.width <= FULL_KEYBOARD_WHITE_KEYS
    ) {
      return { start: stored!.start, width: stored!.width };
    }
  } catch {
    // The default viewport is used when local storage is unavailable or invalid.
  }
  return null;
}

interface DockHeights {
  keyboard?: number;
  pads?: number;
}

function storedDockHeights(): DockHeights {
  try {
    const stored = JSON.parse(
      window.localStorage.getItem(DOCK_HEIGHT_STORAGE_KEY) ?? "null",
    ) as DockHeights | null;
    return {
      keyboard: typeof stored?.keyboard === "number" ? stored.keyboard : undefined,
      pads: typeof stored?.pads === "number" ? stored.pads : undefined,
    };
  } catch {
    return {};
  }
}

function noteName(note: number) {
  return `${NOTE_NAMES[note % 12]}${Math.floor(note / 12) - 1}`;
}

function useControllerSize(ref: RefObject<HTMLElement | null>) {
  const [size, setSize] = useState(() => ({
    width: window.innerWidth,
    height: window.innerHeight,
  }));
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => {
      const bounds = element.getBoundingClientRect();
      setSize({ width: bounds.width, height: bounds.height });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return size;
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

const FULL_KEYBOARD_KEYS = keyboardKeys(FULL_KEYBOARD_START_NOTE, FULL_KEYBOARD_WHITE_KEYS);
const FULL_KEYBOARD_WHITE_NOTES = FULL_KEYBOARD_KEYS
  .filter((key) => !key.black)
  .map((key) => key.note);

function whiteKeyIndexForNote(note: number) {
  const exact = FULL_KEYBOARD_WHITE_NOTES.indexOf(note);
  if (exact >= 0) return exact;
  const next = FULL_KEYBOARD_WHITE_NOTES.findIndex((candidate) => candidate > note);
  return clamp(next < 0 ? FULL_KEYBOARD_WHITE_KEYS - 1 : next, 0, FULL_KEYBOARD_WHITE_KEYS - 1);
}

function noteAtWhitePosition(position: number) {
  return FULL_KEYBOARD_WHITE_NOTES[clamp(
    Math.floor(position),
    0,
    FULL_KEYBOARD_WHITE_KEYS - 1,
  )];
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
  docked = false,
}: {
  snapshot: SessionSnapshot | null;
  connection: string;
  onOpenNavigation: () => void;
  onExit: () => void;
  docked?: boolean;
}) {
  const session = snapshot ?? EMPTY_SESSION;
  const [mode, setMode] = useState<ControllerMode>("keyboard");
  const [octave, setOctave] = useState(3);
  const [velocity, setVelocity] = useState(100);
  const [sustain, setSustain] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [keyboardWidth, setKeyboardWidth] = useState<KeyboardWidth>(storedKeyboardWidth);
  const [keyboardViewport, setKeyboardViewport] = useState<KeyboardViewport | null>(
    storedKeyboardViewport,
  );
  const [padMatrix, setPadMatrix] = useState<PadMatrix>(storedPadMatrix);
  const [dockHeights, setDockHeights] = useState<DockHeights>(storedDockHeights);
  const [resizingDock, setResizingDock] = useState(false);
  const [activeNotes, setActiveNotes] = useState<Set<number>>(() => new Set());
  const controllerRef = useRef<HTMLElement | null>(null);
  const pointerNotes = useRef(new Map<number, number>());
  const notePointers = useRef(new Map<number, Set<number>>());
  const dockResizeRef = useRef<{
    pointerId: number;
    startY: number;
    height: number;
  } | null>(null);
  const keyboardViewportGestureRef = useRef<{
    pointerId: number;
    kind: "move" | "start" | "end";
    startX: number;
    trackWidth: number;
    viewport: KeyboardViewport;
  } | null>(null);
  const controllerSize = useControllerSize(controllerRef);
  const controllerWidth = controllerSize.width;
  const fullKeyboard = keyboardWidth === "auto"
    && controllerWidth >= FULL_KEYBOARD_WHITE_KEYS * FULL_KEYBOARD_MIN_WHITE_KEY_PX;
  const automaticWindowKeys = Math.max(
    8,
    Math.min(MAX_VISIBLE_WHITE_KEYS, Math.floor(controllerWidth / WINDOWED_KEY_MIN_PX)),
  );
  const visibleWhiteKeys = fullKeyboard
    ? FULL_KEYBOARD_WHITE_KEYS
    : keyboardWidth === "auto"
      ? automaticWindowKeys
      : keyboardWidth;
  const keyboardSpan = keyboardKeys(0, visibleWhiteKeys).at(-1)?.note ?? 0;
  const padSpan = padMatrix.rows * padMatrix.columns - 1;
  const maximumOctave = mode === "keyboard" && fullKeyboard
    ? 7
    : maximumOctaveForSpan(mode === "keyboard" ? keyboardSpan : padSpan);
  const windowBaseNote = (octave + 1) * 12;
  const baseNote = mode === "keyboard" && fullKeyboard
    ? FULL_KEYBOARD_START_NOTE
    : windowBaseNote;
  const playable = connection === "online" && session.active_mode !== "idle";
  const sizingHeight = docked ? window.innerHeight : controllerSize.height;
  const portraitDock = docked && controllerWidth <= 760 && sizingHeight > controllerWidth;
  const portraitKeyboardNavigator = portraitDock && mode === "keyboard" && !fullKeyboard;
  const minimumKeyboardViewportWidth = Math.min(
    FULL_KEYBOARD_WHITE_KEYS,
    visibleWhiteKeys,
  );
  const defaultKeyboardViewport = useMemo<KeyboardViewport>(() => {
    const width = minimumKeyboardViewportWidth;
    return {
      start: clamp(
        whiteKeyIndexForNote(windowBaseNote),
        0,
        FULL_KEYBOARD_WHITE_KEYS - width,
      ),
      width,
    };
  }, [minimumKeyboardViewportWidth, windowBaseNote]);
  const resolvedKeyboardViewport = useMemo<KeyboardViewport>(() => {
    if (!keyboardViewport) return defaultKeyboardViewport;
    const width = clamp(
      keyboardViewport.width,
      minimumKeyboardViewportWidth,
      FULL_KEYBOARD_WHITE_KEYS,
    );
    return {
      start: clamp(keyboardViewport.start, 0, FULL_KEYBOARD_WHITE_KEYS - width),
      width,
    };
  }, [defaultKeyboardViewport, keyboardViewport, minimumKeyboardViewportWidth]);
  const minimumDockHeight = mode === "keyboard"
    ? portraitDock ? 200 : 190
    : portraitDock ? 190 : 250;
  const maximumDockHeight = mode === "keyboard"
    ? Math.max(
      minimumDockHeight,
      Math.min(440, sizingHeight * (portraitDock ? 0.55 : 0.48)),
    )
    : Math.max(
      minimumDockHeight,
      portraitDock ? Math.min(540, sizingHeight * 0.66) : sizingHeight - 80,
    );
  const automaticDockHeight = mode === "keyboard"
    ? Math.min(300, Math.max(minimumDockHeight, sizingHeight * (portraitDock ? 0.29 : 0.3)))
    : Math.min(
      portraitDock ? 420 : 560,
      Math.max(minimumDockHeight, sizingHeight * (portraitDock ? 0.44 : 0.52)),
    );
  const dockHeight = Math.min(
    maximumDockHeight,
    Math.max(minimumDockHeight, dockHeights[mode] ?? automaticDockHeight),
  );
  const roomyLayout = window.innerWidth >= 1100 && window.innerHeight >= 620;
  const padGap = roomyLayout ? 9 : 7;
  const padAreaWidth = Math.max(
    1,
    Math.min(920, controllerSize.width - (roomyLayout ? 28 : 14)),
  );
  const dockChromeHeight = docked ? roomyLayout ? 48 : 42 : 0;
  const padSizingHeight = docked ? dockHeight - dockChromeHeight : controllerSize.height;
  const padAreaHeight = Math.max(
    1,
    padSizingHeight - (roomyLayout ? 28 : 14),
  );
  const padCellHeight = Math.max(1, Math.min(
    (padAreaHeight - padGap * (padMatrix.rows - 1)) / padMatrix.rows,
    (padAreaWidth - padGap * (padMatrix.columns - 1)) /
      padMatrix.columns / PAD_ASPECT_RATIO,
  ));
  const padCellWidth = padCellHeight * PAD_ASPECT_RATIO;
  const padGridWidth = padCellWidth * padMatrix.columns + padGap * (padMatrix.columns - 1);
  const padGridHeight = padCellHeight * padMatrix.rows + padGap * (padMatrix.rows - 1);
  const padDensityClass = padCellHeight < 54
    ? " micro"
    : padCellHeight < 92
      ? " compact"
      : padCellHeight < 132
        ? " condensed"
        : "";

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
    if (!keyboardViewport) return;
    try {
      window.localStorage.setItem(
        KEYBOARD_VIEWPORT_STORAGE_KEY,
        JSON.stringify(resolvedKeyboardViewport),
      );
    } catch {
      // The navigator remains usable when persistent storage is unavailable.
    }
  }, [keyboardViewport, resolvedKeyboardViewport]);

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
    try {
      window.localStorage.setItem(DOCK_HEIGHT_STORAGE_KEY, JSON.stringify(dockHeights));
    } catch {
      // Persistent sizing is optional in hardened or ephemeral WebViews.
    }
  }, [dockHeights]);

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
    if (next === "pads" || !fullKeyboard) {
      const nextSpan = next === "keyboard" ? keyboardSpan : padSpan;
      setOctave((current) => Math.min(current, maximumOctaveForSpan(nextSpan)));
    }
    setMode(next);
  };

  const changeOctave = (amount: number) => {
    panic();
    setKeyboardViewport(null);
    setOctave((current) => Math.max(1, Math.min(maximumOctave, current + amount)));
  };

  const setResolvedKeyboardViewport = useCallback((viewport: KeyboardViewport) => {
    const width = clamp(
      viewport.width,
      minimumKeyboardViewportWidth,
      FULL_KEYBOARD_WHITE_KEYS,
    );
    setKeyboardViewport({
      start: clamp(viewport.start, 0, FULL_KEYBOARD_WHITE_KEYS - width),
      width,
    });
  }, [minimumKeyboardViewportWidth]);

  const beginKeyboardViewportGesture = (
    event: ReactPointerEvent<HTMLButtonElement>,
    kind: "move" | "start" | "end",
  ) => {
    if (event.button !== 0) return;
    const track = event.currentTarget.closest<HTMLElement>(".touch-keyboard-navigator-track");
    if (!track) return;
    event.preventDefault();
    event.stopPropagation();
    panic();
    event.currentTarget.setPointerCapture(event.pointerId);
    keyboardViewportGestureRef.current = {
      pointerId: event.pointerId,
      kind,
      startX: event.clientX,
      trackWidth: Math.max(1, track.getBoundingClientRect().width),
      viewport: resolvedKeyboardViewport,
    };
    setKeyboardViewport(resolvedKeyboardViewport);
    hostHaptic("confirm");
  };

  const moveKeyboardViewportGesture = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const gesture = keyboardViewportGestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    const delta = (event.clientX - gesture.startX) /
      gesture.trackWidth * FULL_KEYBOARD_WHITE_KEYS;
    const initialEnd = gesture.viewport.start + gesture.viewport.width;
    if (gesture.kind === "move") {
      setResolvedKeyboardViewport({
        ...gesture.viewport,
        start: gesture.viewport.start + delta,
      });
    } else if (gesture.kind === "start") {
      const nextStart = clamp(
        gesture.viewport.start + delta,
        0,
        initialEnd - minimumKeyboardViewportWidth,
      );
      setResolvedKeyboardViewport({ start: nextStart, width: initialEnd - nextStart });
    } else {
      const nextEnd = clamp(
        initialEnd + delta,
        gesture.viewport.start + minimumKeyboardViewportWidth,
        FULL_KEYBOARD_WHITE_KEYS,
      );
      setResolvedKeyboardViewport({
        start: gesture.viewport.start,
        width: nextEnd - gesture.viewport.start,
      });
    }
  };

  const finishKeyboardViewportGesture = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (keyboardViewportGestureRef.current?.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    keyboardViewportGestureRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const toggleSustain = () => {
    if (!playable) return;
    const next = !sustain;
    if (!sendVirtualMidi(0xb0, 64, next ? 127 : 0)) return;
    setSustain(next);
    hostHaptic("confirm");
  };

  const setCurrentDockHeight = useCallback((height: number | undefined) => {
    setDockHeights((current) => ({ ...current, [mode]: height }));
  }, [mode]);

  const beginDockResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    dockResizeRef.current = {
      pointerId: event.pointerId,
      startY: event.clientY,
      height: dockHeight,
    };
    setResizingDock(true);
    hostHaptic("confirm");
  };

  const resizeDock = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const gesture = dockResizeRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    setCurrentDockHeight(Math.min(
      maximumDockHeight,
      Math.max(minimumDockHeight, gesture.height + gesture.startY - event.clientY),
    ));
  };

  const finishDockResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (dockResizeRef.current?.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    dockResizeRef.current = null;
    setResizingDock(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const resizeDockWithKeyboard = (event: KeyboardEvent<HTMLButtonElement>) => {
    const direction = event.key === "ArrowUp" ? 1 : event.key === "ArrowDown" ? -1 : 0;
    if (!direction) return;
    event.preventDefault();
    event.stopPropagation();
    const step = event.shiftKey ? 40 : 10;
    setCurrentDockHeight(Math.min(
      maximumDockHeight,
      Math.max(minimumDockHeight, dockHeight + direction * step),
    ));
  };

  const keys = useMemo(
    () => keyboardKeys(baseNote, visibleWhiteKeys),
    [baseNote, visibleWhiteKeys],
  );
  const displayedKeyboardKeys = portraitKeyboardNavigator ? FULL_KEYBOARD_KEYS : keys;
  const viewportStartName = noteName(noteAtWhitePosition(resolvedKeyboardViewport.start));
  const viewportEndName = noteName(noteAtWhitePosition(
    resolvedKeyboardViewport.start + resolvedKeyboardViewport.width - 0.000001,
  ));
  const pads = useMemo(() => padNotes(baseNote, padMatrix), [baseNote, padMatrix]);

  return (
    <section
      ref={controllerRef}
      className={`touch-controller mode-${mode}${fullKeyboard ? " full-keyboard" : ""}${
        docked ? " docked" : ""
      }${portraitKeyboardNavigator ? " keyboard-navigator-active" : ""
      }`}
      style={{ "--controller-dock-height": `${dockHeight}px` } as CSSProperties}
      onPointerMove={pointerMove}
      onPointerUp={(event) => releasePointer(event.pointerId)}
      onPointerCancel={(event) => releasePointer(event.pointerId)}
      onContextMenu={(event) => event.preventDefault()}
    >
      {!docked ? <button
        className="touch-controller-menu-button"
        onClick={() => setMenuOpen(true)}
        aria-label="Open controller menu"
        aria-expanded={menuOpen}
      >
        <Menu aria-hidden="true" />
      </button> : null}

      {!docked ? <div className="touch-controller-stage">
        <span>TOUCH CONTROLLER</span>
        <strong>{targetName}</strong>
        <small>{playable ? targetDetail : "The audio host is not ready"}</small>
      </div> : null}

      <div className={`touch-controller-dock${resizingDock ? " resizing" : ""}`}>
      <button
        type="button"
        className="touch-controller-dock-resizer"
        role="separator"
        aria-label={`Resize ${mode} dock`}
        aria-orientation="horizontal"
        aria-valuemin={Math.round(minimumDockHeight)}
        aria-valuemax={Math.round(maximumDockHeight)}
        aria-valuenow={Math.round(dockHeight)}
        title="Drag to resize · Double-click to restore automatic height"
        onPointerDown={beginDockResize}
        onPointerMove={resizeDock}
        onPointerUp={finishDockResize}
        onPointerCancel={finishDockResize}
        onLostPointerCapture={() => {
          dockResizeRef.current = null;
          setResizingDock(false);
        }}
        onDoubleClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          setCurrentDockHeight(undefined);
        }}
        onKeyDown={resizeDockWithKeyboard}
      >
        <span />
      </button>
      {portraitKeyboardNavigator ? (
        <div className="touch-keyboard-navigator" aria-label="Visible keyboard range">
          <span className="touch-keyboard-navigator-label">A0</span>
          <div className="touch-keyboard-navigator-track">
            <div className="touch-keyboard-navigator-whites" aria-hidden="true" />
            {FULL_KEYBOARD_KEYS.filter((key) => key.black).map((key) => (
              <span
                key={key.note}
                className="touch-keyboard-navigator-black"
                style={{ left: `${key.left}%` }}
                aria-hidden="true"
              />
            ))}
            <div
              className="touch-keyboard-navigator-window"
              style={{
                left: `${resolvedKeyboardViewport.start / FULL_KEYBOARD_WHITE_KEYS * 100}%`,
                width: `${resolvedKeyboardViewport.width / FULL_KEYBOARD_WHITE_KEYS * 100}%`,
              }}
            >
              <button
                type="button"
                className="touch-keyboard-navigator-handle start"
                aria-label="Expand visible keyboard range from the left"
                onPointerDown={(event) => beginKeyboardViewportGesture(event, "start")}
                onPointerMove={moveKeyboardViewportGesture}
                onPointerUp={finishKeyboardViewportGesture}
                onPointerCancel={finishKeyboardViewportGesture}
                onLostPointerCapture={() => { keyboardViewportGestureRef.current = null; }}
              ><span /></button>
              <button
                type="button"
                className="touch-keyboard-navigator-move"
                aria-label={`Move visible keyboard range, ${viewportStartName} to ${viewportEndName}`}
                onPointerDown={(event) => beginKeyboardViewportGesture(event, "move")}
                onPointerMove={moveKeyboardViewportGesture}
                onPointerUp={finishKeyboardViewportGesture}
                onPointerCancel={finishKeyboardViewportGesture}
                onLostPointerCapture={() => { keyboardViewportGestureRef.current = null; }}
              ><span /></button>
              <button
                type="button"
                className="touch-keyboard-navigator-handle end"
                aria-label="Expand visible keyboard range from the right"
                onPointerDown={(event) => beginKeyboardViewportGesture(event, "end")}
                onPointerMove={moveKeyboardViewportGesture}
                onPointerUp={finishKeyboardViewportGesture}
                onPointerCancel={finishKeyboardViewportGesture}
                onLostPointerCapture={() => { keyboardViewportGestureRef.current = null; }}
              ><span /></button>
            </div>
          </div>
          <span className="touch-keyboard-navigator-label">C8</span>
        </div>
      ) : null}
      <div className="touch-controller-dockbar">
        <div className="touch-mode-switch" aria-label="Touch controller layout">
          <button className={mode === "keyboard" ? "active" : ""} onClick={() => changeMode("keyboard")}>
            <Piano aria-hidden="true" /> Keyboard
          </button>
          <button className={mode === "pads" ? "active" : ""} onClick={() => changeMode("pads")}>
            <Grid3X3 aria-hidden="true" /> Pads
          </button>
        </div>
        <span className="touch-controller-range-summary">
          {mode === "keyboard"
            ? fullKeyboard ? "88 KEYS · A0—C8" : `${visibleWhiteKeys} WHITE KEYS · FROM C${octave}`
            : `${padMatrix.columns} × ${padMatrix.rows} PADS · FROM C${octave}`}
        </span>
        <button className={sustain ? "active" : ""} onClick={toggleSustain} disabled={!playable}>Sustain</button>
        <button onClick={panic}>Panic</button>
        <button onClick={() => setMenuOpen(true)}><Menu aria-hidden="true" /> Settings</button>
      </div>

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

            {(mode === "pads" || (!fullKeyboard && !portraitKeyboardNavigator)) ? <div className="touch-octave-control">
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
            </div> : null}

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

            {mode === "keyboard" && !fullKeyboard && !portraitKeyboardNavigator ? <div className="touch-key-width-control">
              <div>
                <span>VISIBLE WHITE KEYS</span>
                <output>{keyboardWidth === "auto" ? `AUTO · ${visibleWhiteKeys}` : visibleWhiteKeys}</output>
                <button
                  className={keyboardWidth === "auto" ? "active" : ""}
                  onClick={() => {
                    panic();
                    setKeyboardViewport(null);
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
                  setKeyboardViewport(null);
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
              {docked ? "Close Touch Controller" : "Exit Touch Controller"}
            </button>
          </aside>
        </div>
      ) : null}

      <div className={`touch-instrument${playable ? "" : " disabled"}`}>
        {mode === "keyboard" ? (
          <div
            className={`piano-keyboard${portraitKeyboardNavigator ? " viewport-windowed" : ""}`}
            style={{
              "--white-key-count": portraitKeyboardNavigator
                ? FULL_KEYBOARD_WHITE_KEYS
                : visibleWhiteKeys,
              "--keyboard-track-scale": portraitKeyboardNavigator
                ? FULL_KEYBOARD_WHITE_KEYS / resolvedKeyboardViewport.width
                : 1,
              "--keyboard-track-offset": portraitKeyboardNavigator
                ? resolvedKeyboardViewport.start / FULL_KEYBOARD_WHITE_KEYS * 100
                : 0,
            } as CSSProperties}
            aria-label="On-screen piano keyboard"
          >
            <div className="piano-keyboard-track">
            <div className="piano-white-keys">
              {displayedKeyboardKeys.filter((key) => !key.black).map((key) => (
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
            {displayedKeyboardKeys.filter((key) => key.black).map((key) => (
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
          </div>
        ) : (
          <div
            className={`touch-pad-grid fitted${padDensityClass}`}
            style={{
              "--pad-columns": padMatrix.columns,
              "--pad-rows": padMatrix.rows,
              "--pad-gap": `${padGap}px`,
              "--pad-grid-width": `${padGridWidth}px`,
              "--pad-grid-height": `${padGridHeight}px`,
            } as CSSProperties}
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
      </div>
    </section>
  );
}
