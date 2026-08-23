import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import {
  beginMidiLearn,
  cancelMidiLearn,
  removeParameterLink,
  requestMidiLearnStatus,
  requestMidiSources,
  requestSessionSnapshot,
  upsertParameterLink,
} from "../gateway";
import { randomIdToken } from "../ids";
import type {
  MidiLearnCandidate,
  MidiSourceStatus,
  ParameterLink,
  ParameterLinkMessage,
  PluginParameterSnapshot,
} from "../types";
import { AsyncActionLabel } from "./AsyncSpinner";
import { ModalDialog } from "./ModalDialog";

const PARAMETER_ATTRIBUTE = "data-rackforge-parameter-index";
const LONG_PRESS_MS = 560;
const MOVE_TOLERANCE_PX = 10;

interface Target {
  parameterIndex: number;
  x: number;
  y: number;
}

interface Draft {
  id: string;
  sourceId: string;
  channel: "omni" | string;
  messageType: ParameterLinkMessage["type"];
  number: number;
  invert: boolean;
  passThrough: ParameterLink["pass_through"];
}

function draftFromLink(link?: ParameterLink): Draft {
  const message = link?.message;
  const number = message && ("controller" in message ? message.controller : "note" in message ? message.note : 0);
  return {
    id: link?.id ?? `parameter.${randomIdToken()}`,
    sourceId: link?.source.source_id ?? "",
    channel: link?.channel.mode === "channel" ? String(link.channel.channel) : "omni",
    messageType: message?.type ?? "control_change",
    number: number ?? 0,
    invert: link?.transform.invert ?? false,
    passThrough: link?.pass_through ?? "pass_through",
  };
}

function messageFromDraft(draft: Draft): ParameterLinkMessage {
  switch (draft.messageType) {
    case "control_change": return { type: "control_change", controller: draft.number };
    case "note": return { type: "note", note: draft.number };
    case "poly_pressure": return { type: "poly_pressure", note: draft.number };
    case "pitch_bend": return { type: "pitch_bend" };
    case "channel_pressure": return { type: "channel_pressure" };
  }
}

function applyCandidate(candidate: MidiLearnCandidate, draft: Draft): Draft {
  const number = "controller" in candidate.message
    ? candidate.message.controller
    : "note" in candidate.message
      ? candidate.message.note
      : draft.number;
  return {
    ...draft,
    sourceId: candidate.source.id,
    channel: String(candidate.channel),
    messageType: candidate.message.type,
    number,
  };
}

export function ParameterLinkHost({
  frameRef,
  frameLoaded,
  frameDocumentGeneration,
  instanceId,
  links,
  loadParameters,
  resetParameter,
}: {
  frameRef: RefObject<HTMLIFrameElement | null>;
  frameLoaded: boolean;
  frameDocumentGeneration: number;
  instanceId: string;
  links: ParameterLink[];
  loadParameters: () => Promise<Pick<PluginParameterSnapshot, "schema">>;
  resetParameter: (parameterIndex: number) => Promise<void>;
}) {
  const [target, setTarget] = useState<Target | null>(null);
  const [editing, setEditing] = useState<Target | null>(null);
  const [resetting, setResetting] = useState(false);
  const [menuError, setMenuError] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const highlightRef = useRef<HTMLDivElement | null>(null);
  const activeLink = target
    ? links.find((link) => link.instance_id === instanceId && link.parameter_index === target.parameterIndex)
    : undefined;

  useEffect(() => {
    if (!frameLoaded) return;
    const frame = frameRef.current;
    let document: Document | null = null;
    try {
      document = frame?.contentDocument ?? null;
    } catch {
      return;
    }
    if (!frame || !document) return;
    let gesture: { pointerId: number; x: number; y: number; timer: number } | null = null;
    let suppressNextClick = false;
    let suppressTimer: number | null = null;
    let suppressContextMenuUntil = 0;
    let contextPress: {
      pointerId: number;
      button: number;
      element: HTMLElement;
      x: number;
      y: number;
      opened: boolean;
    } | null = null;

    // DOM constructors are scoped per browsing context. An element created by
    // the plugin iframe is not `instanceof Element` from RackForge's parent
    // window, even though it is a perfectly valid DOM element.
    const markedTarget = (eventTarget: EventTarget | null) => {
      const element = eventTarget as Element | null;
      return typeof element?.closest === "function"
        ? element.closest<HTMLElement>(`[${PARAMETER_ATTRIBUTE}]`)
        : null;
    };
    const clearContextPress = () => {
      contextPress?.element.classList.remove("rackforge-context-press");
      contextPress = null;
    };
    const open = (element: HTMLElement, clientX: number, clientY: number) => {
      const parameterIndex = Number(element.getAttribute(PARAMETER_ATTRIBUTE));
      if (!Number.isInteger(parameterIndex) || parameterIndex < 0) return;
      const bounds = frame.getBoundingClientRect();
      highlightRef.current?.remove();
      const highlight = document.createElement("div");
      const targetBounds = element.getBoundingClientRect();
      highlight.setAttribute("aria-hidden", "true");
      highlight.className = "rackforge-parameter-highlight";
      highlight.style.cssText = [
        "position:fixed",
        `left:${targetBounds.left - 3}px`,
        `top:${targetBounds.top - 3}px`,
        `width:${targetBounds.width + 6}px`,
        `height:${targetBounds.height + 6}px`,
        "box-sizing:border-box",
        "border:2px solid #5cdcf2",
        "border-radius:5px",
        "background:rgba(92,220,242,0.08)",
        "box-shadow:0 0 0 2px rgba(4,16,22,0.72),0 0 14px rgba(92,220,242,0.78)",
        "pointer-events:none",
        "z-index:2147483647",
      ].join(";");
      document.body.append(highlight);
      highlightRef.current = highlight;
      setMenuError(null);
      setTarget({ parameterIndex, x: bounds.left + clientX, y: bounds.top + clientY });
    };
    const contextMenu = (event: MouseEvent) => {
      // The plugin iframe is a separate browsing context, so preventing the
      // browser menu in RackForge's parent document does not cover it.
      // Keep custom plugin events flowing, but never expose the native menu.
      event.preventDefault();
      const element = markedTarget(event.target);
      if (!element) return;
      event.stopPropagation();
      if (contextPress?.element === element) {
        if (!contextPress.opened) {
          open(element, contextPress.x, contextPress.y);
          contextPress.opened = true;
        }
        return;
      }
      // Chromium normally emits `contextmenu` after pointerup. The menu was
      // already opened from the completed press, so this late event must not
      // move it to the release coordinates.
      if (performance.now() <= suppressContextMenuUntil) return;
      open(element, event.clientX, event.clientY);
    };
    const clearGesture = () => {
      if (gesture) window.clearTimeout(gesture.timer);
      gesture = null;
    };
    const pointerDown = (event: globalThis.PointerEvent) => {
      const element = markedTarget(event.target);
      // Any ordinary interaction in the plugin frame is outside the host menu.
      setTarget(null);
      if (!element) return;
      // Context-menu presses belong exclusively to RackForge. Stop them in
      // capture phase so custom plugin controls cannot treat button 2 as an
      // edit before the `contextmenu` event opens the host menu.
      if (event.pointerType === "mouse") {
        if (event.button !== 0) {
          event.preventDefault();
          event.stopImmediatePropagation();
          clearContextPress();
          element.classList.add("rackforge-context-press");
          contextPress = {
            pointerId: event.pointerId,
            button: event.button,
            element,
            x: event.clientX,
            y: event.clientY,
            opened: false,
          };
        }
        return;
      }
      clearGesture();
      const pointerId = event.pointerId;
      const x = event.clientX;
      const y = event.clientY;
      const timer = window.setTimeout(() => {
        if (gesture?.pointerId !== pointerId) return;
        suppressNextClick = true;
        if (suppressTimer !== null) window.clearTimeout(suppressTimer);
        suppressTimer = window.setTimeout(() => { suppressNextClick = false; }, 900);
        open(element, x, y);
        clearGesture();
      }, LONG_PRESS_MS);
      gesture = { pointerId, x, y, timer };
    };
    const pointerMove = (event: globalThis.PointerEvent) => {
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      if (Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) > MOVE_TOLERANCE_PX) {
        clearGesture();
      }
    };
    const pointerEnd = (event: globalThis.PointerEvent) => {
      if (event.pointerType === "mouse" && event.button !== 0) {
        const completed = contextPress?.pointerId === event.pointerId
          ? contextPress
          : null;
        if (completed && completed.button === 2 && !completed.opened) {
          open(completed.element, completed.x, completed.y);
          completed.opened = true;
        }
        clearContextPress();
        suppressContextMenuUntil = performance.now() + 500;
        event.preventDefault();
        event.stopImmediatePropagation();
        return;
      }
      if (gesture?.pointerId === event.pointerId) clearGesture();
    };
    const click = (event: MouseEvent) => {
      if (event.button !== 0 && markedTarget(event.target)) {
        event.preventDefault();
        event.stopImmediatePropagation();
        return;
      }
      if (!suppressNextClick || !markedTarget(event.target)) return;
      suppressNextClick = false;
      if (suppressTimer !== null) window.clearTimeout(suppressTimer);
      suppressTimer = null;
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    document.addEventListener("contextmenu", contextMenu, true);
    document.addEventListener("auxclick", click, true);
    document.addEventListener("pointerdown", pointerDown, true);
    document.addEventListener("pointermove", pointerMove, true);
    document.addEventListener("pointerup", pointerEnd, true);
    document.addEventListener("pointercancel", pointerEnd, true);
    document.addEventListener("click", click, true);
    return () => {
      clearGesture();
      clearContextPress();
      highlightRef.current?.remove();
      highlightRef.current = null;
      if (suppressTimer !== null) window.clearTimeout(suppressTimer);
      document?.removeEventListener("contextmenu", contextMenu, true);
      document?.removeEventListener("auxclick", click, true);
      document?.removeEventListener("pointerdown", pointerDown, true);
      document?.removeEventListener("pointermove", pointerMove, true);
      document?.removeEventListener("pointerup", pointerEnd, true);
      document?.removeEventListener("pointercancel", pointerEnd, true);
      document?.removeEventListener("click", click, true);
    };
  }, [frameDocumentGeneration, frameLoaded, frameRef]);

  useEffect(() => {
    if (target) return;
    highlightRef.current?.remove();
    highlightRef.current = null;
  }, [target]);

  useEffect(() => {
    if (!target) return;
    const close = () => setTarget(null);
    const closeOutside = (event: globalThis.PointerEvent) => {
      const node = event.target as Node | null;
      if (node && menuRef.current?.contains(node)) return;
      close();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("pointerdown", closeOutside, true);
    window.addEventListener("keydown", closeOnEscape, true);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("pointerdown", closeOutside, true);
      window.removeEventListener("keydown", closeOnEscape, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [target]);

  const menuLeft = target
    ? Math.max(8, Math.min(target.x + 4, window.innerWidth - 206))
    : 0;
  const menuHeight = (activeLink ? 132 : 90) + (menuError ? 58 : 0);
  const menuTop = target
    ? Math.max(8, Math.min(target.y + 4, window.innerHeight - menuHeight - 8))
    : 0;

  return (
    <>
      {target ? (
        <div
          ref={menuRef}
          className="parameter-link-context-menu"
          role="menu"
          style={{ left: menuLeft, top: menuTop }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" role="menuitem" onClick={() => { setEditing(target); setTarget(null); }}>
            {activeLink ? "Edit MIDI Link…" : "Link MIDI…"}
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={resetting}
            onClick={() => {
              const parameterIndex = target.parameterIndex;
              setResetting(true);
              setMenuError(null);
              void resetParameter(parameterIndex)
                .then(() => setTarget(null))
                .catch((reason: unknown) => setMenuError(
                  reason instanceof Error ? reason.message : "Could not reset this control.",
                ))
                .finally(() => setResetting(false));
            }}
          >
            {resetting ? "Resetting…" : "Reset to program"}
          </button>
          {activeLink ? (
            <button
              type="button"
              role="menuitem"
              className="danger"
              onClick={() => {
                setTarget(null);
                void removeParameterLink(activeLink.id).then(requestSessionSnapshot);
              }}
            >
              Remove MIDI Link
            </button>
          ) : null}
          {menuError ? <p className="parameter-link-context-error" role="alert">{menuError}</p> : null}
        </div>
      ) : null}
      {editing ? (
        <ParameterLinkDialog
          instanceId={instanceId}
          parameterIndex={editing.parameterIndex}
          existing={links.find((link) => link.instance_id === instanceId && link.parameter_index === editing.parameterIndex)}
          loadParameters={loadParameters}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </>
  );
}

function ParameterLinkDialog({
  instanceId,
  parameterIndex,
  existing,
  loadParameters,
  onClose,
}: {
  instanceId: string;
  parameterIndex: number;
  existing?: ParameterLink;
  loadParameters: () => Promise<Pick<PluginParameterSnapshot, "schema">>;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(() => draftFromLink(existing));
  const [sources, setSources] = useState<MidiSourceStatus[]>([]);
  const [parameterName, setParameterName] = useState(`Parameter ${parameterIndex}`);
  const [busy, setBusy] = useState(true);
  const [learning, setLearning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const learnIdRef = useRef<number | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([requestMidiSources(), loadParameters()])
      .then(([midiSources, snapshot]) => {
        if (!active) return;
        const parameter = snapshot.schema.parameters.find((item) => item.index === parameterIndex);
        if (!parameter) throw new Error(`Plugin parameter ${parameterIndex} no longer exists.`);
        if (parameter.flags.read_only || parameter.kind.type === "meter") {
          throw new Error(`${parameter.name} is read-only and cannot receive MIDI.`);
        }
        setSources(midiSources);
        setParameterName(parameter.name);
        setDraft((current) => ({
          ...current,
          sourceId: current.sourceId || midiSources.find((source) => source.connected)?.source.id || midiSources[0]?.source.id || "",
        }));
      })
      .catch((reason: unknown) => {
        if (active) setError(reason instanceof Error ? reason.message : "Could not prepare MIDI Link.");
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => { active = false; };
  }, [loadParameters, parameterIndex]);

  const stopLearn = useCallback(async () => {
    const learnId = learnIdRef.current;
    learnIdRef.current = null;
    setLearning(false);
    if (learnId !== null) await cancelMidiLearn(learnId).catch(() => undefined);
  }, []);

  useEffect(() => () => { void stopLearn(); }, [stopLearn]);

  const learn = async () => {
    setError(null);
    setLearning(true);
    try {
      const learnId = await beginMidiLearn(instanceId, parameterIndex);
      learnIdRef.current = learnId;
      while (learnIdRef.current === learnId) {
        const candidate = await requestMidiLearnStatus(learnId);
        if (candidate) {
          const approved = await requestMidiSources();
          if (!approved.some((source) => source.source.id === candidate.source.id)) {
            throw new Error("The detected MIDI input is not enabled in Audio & MIDI settings.");
          }
          setSources(approved);
          setDraft((current) => applyCandidate(candidate, current));
          await cancelMidiLearn(learnId).catch(() => undefined);
          learnIdRef.current = null;
          setLearning(false);
          return;
        }
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      }
    } catch (reason) {
      learnIdRef.current = null;
      setLearning(false);
      setError(reason instanceof Error ? reason.message : "MIDI Learn failed.");
    }
  };

  const apply = async () => {
    const source = sources.find((candidate) => candidate.source.id === draft.sourceId);
    if (!source) {
      setError("Choose a MIDI device before applying this link.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await stopLearn();
      await upsertParameterLink({
        schema_version: 1,
        id: draft.id,
        instance_id: instanceId,
        parameter_index: parameterIndex,
        source: { source_id: source.source.id, display_name: source.source.name },
        channel: draft.channel === "omni"
          ? { mode: "omni" }
          : { mode: "channel", channel: Number(draft.channel) },
        message: messageFromDraft(draft),
        transform: { invert: draft.invert },
        pass_through: draft.passThrough,
      });
      await requestSessionSnapshot();
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save MIDI Link.");
    } finally {
      setBusy(false);
    }
  };

  const selectedSource = sources.find((source) => source.source.id === draft.sourceId);
  const numbered = draft.messageType === "control_change" || draft.messageType === "note" || draft.messageType === "poly_pressure";
  return (
    <ModalDialog
      eyebrow="Host parameter mapping"
      title={`MIDI Link · ${parameterName}`}
      className="parameter-link-dialog"
      onClose={() => { void stopLearn().finally(onClose); }}
      dismissible={!busy}
      actions={
        <>
          <button className="secondary-button" type="button" disabled={busy} onClick={() => { void stopLearn().finally(onClose); }}>Cancel</button>
          <button className="primary-button" type="button" disabled={busy || !selectedSource} onClick={() => void apply()}>
            <AsyncActionLabel active={busy} activeLabel="Applying…">Apply</AsyncActionLabel>
          </button>
        </>
      }
    >
      <div className="parameter-link-form">
        {error ? <p className="parameter-link-error" role="alert">{error}</p> : null}
        <label>
          <span>MIDI source</span>
          <select value={selectedSource ? draft.sourceId : ""} disabled={busy || learning} onChange={(event) => setDraft((current) => ({ ...current, sourceId: event.target.value }))}>
            <option value="">Choose a MIDI input</option>
            {sources.map((source) => <option key={source.source.id} value={source.source.id}>{source.source.name}{source.connected ? "" : " · Disconnected"}</option>)}
          </select>
        </label>
        {learning ? <p className="parameter-link-pending">Listening on every enabled MIDI input. The detected source will replace this selection.</p> : null}
        {existing && draft.sourceId && !selectedSource ? <p className="parameter-link-pending">This link's saved MIDI input is disabled. Enable it in Audio & MIDI settings before editing or applying the link.</p> : null}
        {selectedSource && !selectedSource.connected ? <p className="parameter-link-pending">This saved device is disconnected. The link will remain pending and reconnect by its stable identity.</p> : null}
        <div className="parameter-link-grid">
          <label>
            <span>Message</span>
            <select value={draft.messageType} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, messageType: event.target.value as Draft["messageType"] }))}>
              <option value="control_change">Control Change (CC)</option>
              <option value="pitch_bend">Pitch Bend (14-bit)</option>
              <option value="note">Note</option>
              <option value="channel_pressure">Channel Pressure</option>
              <option value="poly_pressure">Poly Pressure</option>
            </select>
          </label>
          <label>
            <span>Channel</span>
            <select value={draft.channel} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, channel: event.target.value }))}>
              <option value="omni">Omni</option>
              {Array.from({ length: 16 }, (_, index) => <option key={index + 1} value={String(index + 1)}>Channel {index + 1}</option>)}
            </select>
          </label>
          {numbered ? (
            <label>
              <span>{draft.messageType === "control_change" ? "CC number" : "Note number"}</span>
              <input type="number" min={0} max={127} value={draft.number} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, number: Math.max(0, Math.min(127, Number(event.target.value))) }))} />
            </label>
          ) : null}
        </div>
        <div className="parameter-link-options">
          <label><input type="checkbox" checked={draft.invert} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, invert: event.target.checked }))} />Invert input</label>
          <label>
            <span>MIDI pass-through</span>
            <select value={draft.passThrough} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, passThrough: event.target.value as ParameterLink["pass_through"] }))}>
              <option value="pass_through">Pass through to instrument</option>
              <option value="consume">Consume message</option>
            </select>
          </label>
        </div>
        <button className={`midi-learn-button${learning ? " learning" : ""}`} type="button" disabled={busy} onClick={() => learning ? void stopLearn() : void learn()}>
          {learning ? "Listening… tap to stop" : "Learn next MIDI message"}
        </button>
        <p className="parameter-link-help">Learn only fills this form. RackForge does not change the project or runtime until you press Apply.</p>
      </div>
    </ModalDialog>
  );
}
