import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { materializePluginState, requestPluginPreset, requestPluginPresets } from "../gateway";
import type { HostPresetSummary, PluginInstance, RackGraphPosition, RackSlot } from "../types";

type PopoverMode = "attached" | "floating";

interface RackSlotPopoverProps {
  slot: RackSlot;
  instance: PluginInstance;
  nodePosition: RackGraphPosition;
  viewport: { x: number; y: number; zoom: number };
  onChange: (slot: RackSlot) => void;
  onClose: () => void;
  renderSurface: (options: {
    instance: PluginInstance;
    onSelectSound: (soundId: string) => Promise<unknown>;
  }) => ReactNode;
}

const POPOVER_OFFSET = { x: 220, y: 0 };
const POPOVER_INITIAL_SIZE = { width: 720, height: 620 };
const POPOVER_MIN_SIZE = { width: 640, height: 520 };
const POPOVER_MAX_SIZE = { width: 1280, height: 920 };

function clampPopoverSize(width: number, height: number) {
  return {
    width: Math.min(POPOVER_MAX_SIZE.width, Math.max(POPOVER_MIN_SIZE.width, width)),
    height: Math.min(POPOVER_MAX_SIZE.height, Math.max(POPOVER_MIN_SIZE.height, height)),
  };
}

export function RackSlotPopover({
  slot,
  instance,
  nodePosition,
  viewport,
  onChange,
  onClose,
  renderSurface,
}: RackSlotPopoverProps) {
  const [mode, setMode] = useState<PopoverMode>("attached");
  const [position, setPosition] = useState<RackGraphPosition>(() => ({
    x: viewport.x + (nodePosition.x + POPOVER_OFFSET.x) * viewport.zoom,
    y: viewport.y + (nodePosition.y + POPOVER_OFFSET.y) * viewport.zoom,
  }));
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [presetId, setPresetId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [size, setSize] = useState(POPOVER_INITIAL_SIZE);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    position: RackGraphPosition;
  } | null>(null);
  const resizeRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    size: typeof POPOVER_INITIAL_SIZE;
  } | null>(null);

  useEffect(() => {
    let active = true;
    requestPluginPresets(slot.plugin_id)
      .then((items) => {
        if (active) setPresets(items);
      })
      .catch((reason: unknown) => {
        if (active) setError(reason instanceof Error ? reason.message : "Could not list presets.");
      });
    return () => {
      active = false;
    };
  }, [slot.plugin_id]);

  const selectSound = useCallback(async (soundId: string) => {
    setBusy(true);
    setError(null);
    try {
      const state = await materializePluginState(slot.plugin_id, soundId);
      onChange({ ...slot, state, legacy_program_id: undefined });
      return { sound_id: soundId, isolated: true };
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : "Could not materialize this sound.";
      setError(message);
      throw new Error(message);
    } finally {
      setBusy(false);
    }
  }, [onChange, slot]);

  const loadPreset = useCallback(async () => {
    if (!presetId) return;
    setBusy(true);
    setError(null);
    try {
      const preset = await requestPluginPreset(slot.plugin_id, presetId);
      onChange({ ...slot, state: preset.state, legacy_program_id: undefined });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not load this preset.");
    } finally {
      setBusy(false);
    }
  }, [onChange, presetId, slot]);

  const beginDrag = (event: PointerEvent<HTMLElement>) => {
    if (event.button !== 0 || event.target instanceof Element && event.target.closest("button, select")) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      position: displayPosition,
    };
  };
  const drag = (event: PointerEvent<HTMLElement>) => {
    const gesture = dragRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    const dx = event.clientX - gesture.startX;
    const dy = event.clientY - gesture.startY;
    if (mode === "attached" && Math.hypot(dx, dy) > 4) setMode("floating");
    setPosition({ x: gesture.position.x + dx, y: gesture.position.y + dy });
  };
  const endDrag = (event: PointerEvent<HTMLElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  };

  const beginResize = (event: PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    resizeRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      size,
    };
  };
  const resize = (event: PointerEvent<HTMLButtonElement>) => {
    const gesture = resizeRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    setSize(clampPopoverSize(
      gesture.size.width + event.clientX - gesture.startX,
      gesture.size.height + event.clientY - gesture.startY,
    ));
  };
  const endResize = (event: PointerEvent<HTMLButtonElement>) => {
    if (resizeRef.current?.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    resizeRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };
  const resizeWithKeyboard = (event: KeyboardEvent<HTMLButtonElement>) => {
    const direction = {
      ArrowLeft: { width: -1, height: 0 },
      ArrowRight: { width: 1, height: 0 },
      ArrowUp: { width: 0, height: -1 },
      ArrowDown: { width: 0, height: 1 },
    }[event.key];
    if (!direction) return;
    event.preventDefault();
    event.stopPropagation();
    const step = event.shiftKey ? 40 : 10;
    setSize((current) => clampPopoverSize(
      current.width + direction.width * step,
      current.height + direction.height * step,
    ));
  };

  const displayPosition = mode === "attached" ? {
    x: viewport.x + (nodePosition.x + POPOVER_OFFSET.x) * viewport.zoom,
    y: viewport.y + (nodePosition.y + POPOVER_OFFSET.y) * viewport.zoom,
  } : position;
  const editorInstance: PluginInstance = {
    ...instance,
    selected_sound_id: slot.state?.selected_sound_id ?? instance.selected_sound_id,
  };
  const anchor = {
    x: viewport.x + (nodePosition.x + 190) * viewport.zoom,
    y: viewport.y + (nodePosition.y + 35) * viewport.zoom,
  };
  return (
    <>
      {mode === "floating" ? (
        <svg className="rack-slot-popover-link" aria-hidden="true">
        <line x1={anchor.x} y1={anchor.y} x2={displayPosition.x} y2={displayPosition.y + 28} />
          <circle cx={anchor.x} cy={anchor.y} r="3" />
        </svg>
      ) : null}
      <section
        className={`rack-slot-popover ${mode}`}
        style={{
          left: displayPosition.x,
          top: displayPosition.y,
          width: size.width,
          height: size.height,
        }}
        aria-label={`Edit ${slot.name}`}
      >
      <span className="rack-slot-popover-tail" aria-hidden="true" />
      <header
        className="rack-slot-popover-header"
        onPointerDown={beginDrag}
        onPointerMove={drag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <div>
          <span>Rack Slot</span>
          <strong>{slot.name}</strong>
        </div>
        <div className="rack-slot-popover-preset">
          <select
            aria-label="RackForge preset"
            value={presetId}
            disabled={busy}
            onChange={(event) => setPresetId(event.target.value)}
          >
            <option value="">{presets.length ? "Choose preset" : "No saved presets"}</option>
            {presets.map((preset) => (
              <option key={preset.id} value={preset.id}>{preset.name}</option>
            ))}
          </select>
          <button type="button" disabled={!presetId || busy} onClick={() => void loadPreset()}>
            Load
          </button>
        </div>
        <button
          type="button"
          className="rack-slot-attach-button"
          onClick={() => setMode((current) => current === "attached" ? "floating" : "attached")}
          title={mode === "attached" ? "Detach from node" : "Attach to node"}
        >
          {mode === "attached" ? "Detach" : "Attach"}
        </button>
        <button type="button" className="rack-slot-close-button" aria-label="Close instrument editor" onClick={onClose}>×</button>
      </header>
      {error ? <div className="rack-slot-popover-error">{error}</div> : null}
      <div className="rack-slot-popover-surface">
        {renderSurface({ instance: editorInstance, onSelectSound: selectSound })}
      </div>
      <footer>
        <span>{mode === "attached" ? "Attached to node" : "Floating locally"}</span>
        <span>{slot.state ? `State v${slot.state.state_version} · ${slot.state.plugin_version}` : "Plugin default state"}</span>
      </footer>
      <button
        type="button"
        className="rack-slot-resize-handle"
        aria-label="Resize instrument editor"
        title="Drag to resize · Arrow keys for fine adjustment"
        onPointerDown={beginResize}
        onPointerMove={resize}
        onPointerUp={endResize}
        onPointerCancel={endResize}
        onKeyDown={resizeWithKeyboard}
      />
      </section>
    </>
  );
}
