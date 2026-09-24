import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import {
  type ControllerPackageSummary,
  defaultMode,
  emptyControllerMap,
  learntInput,
  MODE_HINTS,
  MODE_LABELS,
  mappingsForParameter,
  modeProblem,
  modesFor,
  newMappingId,
  suggestedMode,
  withMapping,
  withoutMapping,
} from "../controllerMapping";
import {
  beginMidiLearn,
  cancelMidiLearn,
  removeParameterLink,
  requestControllerMaps,
  requestMidiLearnStatus,
  requestMidiSources,
  requestSessionSnapshot,
  saveControllerMap,
  upsertParameterLink,
} from "../gateway";
import { hostJson, IS_BROWSER_HOST, isNativeHost } from "../host";
import { randomIdToken } from "../ids";
import type {
  ControlMapping,
  ControllerMap,
  MidiLearnCandidate,
  MidiSourceStatus,
  ParameterLink,
  ParameterLinkMessage,
  ParameterLinkMode,
  PluginParameterDescriptor,
  PluginParameterSnapshot,
  RegisteredController,
} from "../types";
import { AsyncActionLabel } from "./AsyncSpinner";
import { ModalDialog } from "./ModalDialog";
import { ModeFields } from "./controllers/ModeFields";

/** The controllers attached here, their maps and their packages' controls. */
interface ControllerData {
  controllers: RegisteredController[];
  maps: ControllerMap[];
  packages: ControllerPackageSummary[];
}

async function loadControllerData(): Promise<ControllerData> {
  const [maps, packages] = await Promise.all([
    requestControllerMaps(),
    hostJson<{ controllers?: ControllerPackageSummary[] }>("/api/v1/controllers")
      .then((response) => response.controllers ?? [])
      .catch(() => []),
  ]);
  return { controllers: maps.controllers, maps: maps.maps, packages };
}

/** Only a host that keeps controller maps is offered one. */
function hostKeepsMaps() {
  return !IS_BROWSER_HOST && !isNativeHost();
}

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

/** A draft of a controller map's mapping, heard on its controller's input. */
function draftFromMapping(mapping: ControlMapping, sourceId: string): Draft {
  const message = mapping.input.message;
  return {
    id: `parameter.${randomIdToken()}`,
    sourceId,
    channel: mapping.input.channel.mode === "channel" ? String(mapping.input.channel.channel) : "omni",
    messageType: message.type,
    number: "controller" in message ? message.controller : "note" in message ? message.note : 0,
    invert: mapping.invert ?? false,
    passThrough: mapping.pass_through ?? "pass_through",
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
  pluginId,
  pluginName,
  links,
  loadParameters,
  resetParameter,
}: {
  frameRef: RefObject<HTMLIFrameElement | null>;
  frameLoaded: boolean;
  frameDocumentGeneration: number;
  instanceId: string;
  pluginId?: string;
  pluginName?: string;
  links: ParameterLink[];
  loadParameters: () => Promise<Pick<PluginParameterSnapshot, "schema">>;
  resetParameter: (parameterIndex: number) => Promise<void>;
}) {
  const [target, setTarget] = useState<Target | null>(null);
  const [editing, setEditing] = useState<Target | null>(null);
  const [resetting, setResetting] = useState(false);
  const [menuError, setMenuError] = useState<string | null>(null);
  // The controller maps, asked for when a menu opens: what already drives
  // the parameter from a controller, and where a learnt control is kept.
  const [controllerData, setControllerData] = useState<ControllerData | null>(null);
  // The map's mapping the dialog opens on, when no session link is there.
  const [editingMapped, setEditingMapped] = useState<{ controller_id: string; mapping: ControlMapping } | null>(null);
  // The id of the parameter a menu is open on, with the index it was read for.
  const [targetParameter, setTargetParameter] = useState<{ index: number; id: string | null } | null>(null);
  const targetParameterId = target && targetParameter?.index === target.parameterIndex ? targetParameter.id : null;
  const menuRef = useRef<HTMLDivElement | null>(null);
  const highlightRef = useRef<HTMLDivElement | null>(null);
  const activeLink = target
    ? links.find((link) => link.instance_id === instanceId && link.parameter_index === target.parameterIndex)
    : undefined;
  const mapsHere = Boolean(pluginId) && hostKeepsMaps();
  const mappedHere = target && pluginId && targetParameterId && controllerData
    ? mappingsForParameter(controllerData.maps, pluginId, targetParameterId)
    : [];

  const refreshControllers = useCallback(async () => {
    if (!mapsHere) return;
    setControllerData(await loadControllerData());
  }, [mapsHere]);

  useEffect(() => {
    if (!target || !mapsHere) return;
    let active = true;
    loadParameters()
      .then((snapshot) => {
        if (!active) return;
        const parameter = snapshot.schema.parameters.find((item) => item.index === target.parameterIndex);
        setTargetParameter({ index: target.parameterIndex, id: parameter?.id ?? null });
      })
      .catch(() => undefined);
    loadControllerData()
      .then((data) => {
        if (active) setControllerData(data);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [loadParameters, mapsHere, target]);

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
      // Drawn inside the plugin's document, where RackForge's stylesheets do
      // not reach: the input's colour from the colour code (design/tokens.css)
      // is read here and written in -- a link is MIDI, and MIDI is input.
      const accent = getComputedStyle(window.document.documentElement)
        .getPropertyValue("--rf-color-input").trim() || "#5b3a8e";
      highlight.style.cssText = [
        "position:fixed",
        `left:${targetBounds.left - 3}px`,
        `top:${targetBounds.top - 3}px`,
        `width:${targetBounds.width + 6}px`,
        `height:${targetBounds.height + 6}px`,
        "box-sizing:border-box",
        `border:2px solid ${accent}`,
        "border-radius:5px",
        `background:color-mix(in srgb, ${accent} 8%, transparent)`,
        `box-shadow:0 0 0 2px rgba(0,0,0,0.45),0 0 14px color-mix(in srgb, ${accent} 70%, transparent)`,
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
  const menuHeight = (activeLink ? 132 : 90) + mappedHere.length * 42 + (menuError ? 58 : 0);
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
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setEditingMapped(activeLink ? null : mappedHere[0] ?? null);
              setEditing(target);
              setTarget(null);
            }}
          >
            {activeLink || mappedHere.length > 0 ? "Edit MIDI Link…" : "Link MIDI…"}
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
          {mappedHere.map((entry) => (
            <button
              key={`${entry.controller_id}:${entry.mapping.id}`}
              type="button"
              role="menuitem"
              className="danger"
              onClick={() => {
                const map = controllerData?.maps.find((candidate) => candidate.controller_id === entry.controller_id);
                if (!map || !pluginId) return;
                setMenuError(null);
                saveControllerMap(withoutMapping(map, pluginId, entry.mapping.id))
                  .then(() => refreshControllers())
                  .then(() => setTarget(null))
                  .catch((reason: unknown) => setMenuError(
                    reason instanceof Error ? reason.message : "Could not remove this mapping.",
                  ));
              }}
            >
              Remove {entry.controller_name} · {entry.mapping.input.name}
            </button>
          ))}
          {menuError ? <p className="parameter-link-context-error" role="alert">{menuError}</p> : null}
        </div>
      ) : null}
      {editing ? (
        <ParameterLinkDialog
          instanceId={instanceId}
          parameterIndex={editing.parameterIndex}
          existing={links.find((link) => link.instance_id === instanceId && link.parameter_index === editing.parameterIndex)}
          loadParameters={loadParameters}
          plugin={mapsHere && pluginId ? { plugin_id: pluginId, plugin_name: pluginName ?? pluginId } : undefined}
          controllerData={controllerData}
          mapped={editingMapped ?? undefined}
          onSaved={() => refreshControllers().catch(() => undefined)}
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
  plugin,
  controllerData,
  mapped,
  onSaved,
  onClose,
}: {
  instanceId: string;
  parameterIndex: number;
  existing?: ParameterLink;
  loadParameters: () => Promise<Pick<PluginParameterSnapshot, "schema">>;
  /** The plugin the parameter belongs to, when its controls can go in a controller map. */
  plugin?: { plugin_id: string; plugin_name: string };
  controllerData: ControllerData | null;
  /** The controller map's mapping being edited, when there is no session link. */
  mapped?: { controller_id: string; mapping: ControlMapping };
  onSaved: () => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(() => {
    const mappedSource = mapped
      ? controllerData?.controllers.find((candidate) => candidate.controller_id === mapped.controller_id)?.source?.id
      : undefined;
    return mapped && mappedSource ? draftFromMapping(mapped.mapping, mappedSource) : draftFromLink(existing);
  });
  const [sources, setSources] = useState<MidiSourceStatus[]>([]);
  const [parameterName, setParameterName] = useState(`Parameter ${parameterIndex}`);
  const [parameter, setParameter] = useState<PluginParameterDescriptor | null>(null);
  const [busy, setBusy] = useState(true);
  const [learning, setLearning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A link learnt in this session stays one until the player says otherwise;
  // a new one goes in the controller's map, where it lasts.
  const [saveIn, setSaveIn] = useState<"map" | "session">(existing ? "session" : "map");
  const [mode, setMode] = useState<ParameterLinkMode | null>(existing?.mode ?? mapped?.mapping.mode ?? null);
  const [isButton, setIsButton] = useState(false);
  const [passThroughTouched, setPassThroughTouched] = useState(
    Boolean(existing) || mapped?.mapping.pass_through !== undefined,
  );
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
        setParameter(parameter);
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

  // The controller the chosen input belongs to, when a package claims it:
  // its map is where a learnt control can be kept for every session.
  const controller = plugin && controllerData
    ? controllerData.controllers.find((candidate) => candidate.source?.id === draft.sourceId)
    : undefined;
  const controllerPackage = controller
    ? controllerData?.packages.find((candidate) => candidate.id === controller.controller_id)
    : undefined;
  const learnt = learntInput(
    messageFromDraft(draft),
    draft.channel === "omni" ? 1 : Number(draft.channel),
    controllerPackage?.inputs ?? [],
  );
  const namedByPackage = Boolean(controllerPackage?.inputs?.some((input) => input.id === learnt.input.id));
  const kind = !namedByPackage && draft.messageType === "control_change" && isButton ? "button" : learnt.kind;
  const toMap = saveIn === "map" && Boolean(controller) && Boolean(plugin);
  const effectiveMode = parameter
    ? mode && modeProblem({ kind }, parameter, mode) === null
      ? mode
      : mode && modesFor({ kind }, parameter).includes(mode.kind)
        ? mode
        : suggestedMode({ kind }, parameter)
    : null;
  const problem = parameter && effectiveMode ? modeProblem({ kind }, parameter, effectiveMode) : null;

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
      if (toMap && controller && plugin && parameter && effectiveMode) {
        const map = controllerData?.maps.find((candidate) => candidate.controller_id === controller.controller_id)
          ?? emptyControllerMap(
            controller.controller_id,
            controllerPackage?.name ?? controller.source?.name ?? controller.controller_id,
          );
        const input: ControlMapping["input"] = draft.channel === "omni"
          ? { ...learnt.input, channel: { mode: "omni" } }
          : learnt.input;
        // The same control on the same parameter keeps its mapping's id.
        const previous = map.plugins
          .find((entry) => entry.plugin_id === plugin.plugin_id)
          ?.mappings.find((mapping) => mapping.input.id === input.id && mapping.parameter_id === parameter.id);
        await saveControllerMap(withMapping(map, plugin, {
          id: previous?.id ?? newMappingId(),
          input,
          parameter_id: parameter.id,
          mode: effectiveMode,
          ...(draft.invert ? { invert: true } : {}),
          ...(passThroughTouched ? { pass_through: draft.passThrough } : {}),
        }));
        // A session link on this parameter would win over the map; the
        // player chose the map.
        if (existing) await removeParameterLink(existing.id);
        onSaved();
      } else {
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
          ...(effectiveMode && effectiveMode.kind !== "direct" ? { mode: effectiveMode } : {}),
        });
      }
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
          <button className="primary-button" type="button" disabled={busy || !selectedSource || problem !== null} onClick={() => void apply()}>
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
            <select
              value={draft.passThrough}
              disabled={busy}
              onChange={(event) => {
                setPassThroughTouched(true);
                setDraft((current) => ({ ...current, passThrough: event.target.value as ParameterLink["pass_through"] }));
              }}
            >
              <option value="pass_through">Pass through to instrument</option>
              <option value="consume">Consume message</option>
            </select>
          </label>
        </div>
        {controller && plugin ? (
          <fieldset className="parameter-link-destination">
            <legend>Save in</legend>
            <label>
              <input type="radio" name="parameter-link-destination" checked={saveIn === "map"} disabled={busy} onChange={() => setSaveIn("map")} />
              <span>
                {controllerPackage?.name ?? controller.source?.name ?? "The controller"}'s map · every session, whenever {plugin.plugin_name} plays
              </span>
            </label>
            <label>
              <input type="radio" name="parameter-link-destination" checked={saveIn === "session"} disabled={busy} onChange={() => setSaveIn("session")} />
              <span>This session only · this instance</span>
            </label>
          </fieldset>
        ) : null}
        {parameter && effectiveMode ? (
          <fieldset className="controller-mode-picker parameter-link-mode">
            <legend>Mode</legend>
            {!namedByPackage && draft.messageType === "control_change" ? (
              <label className="parameter-link-kind">
                <span>The control is a</span>
                <select value={isButton ? "button" : "continuous"} disabled={busy} onChange={(event) => { setIsButton(event.target.value === "button"); setMode(null); }}>
                  <option value="continuous">Knob or fader</option>
                  <option value="button">Button</option>
                </select>
              </label>
            ) : null}
            <div className="controller-mode-keys" role="radiogroup" aria-label="Mode">
              {modesFor({ kind }, parameter).map((candidate) => (
                <button
                  key={candidate}
                  type="button"
                  role="radio"
                  aria-checked={effectiveMode.kind === candidate}
                  className={effectiveMode.kind === candidate ? "active" : undefined}
                  disabled={busy}
                  onClick={() => setMode(candidate === effectiveMode.kind ? effectiveMode : defaultMode(candidate, parameter))}
                >
                  {MODE_LABELS[candidate]}
                </button>
              ))}
            </div>
            <p className="parameter-link-help">{MODE_HINTS[effectiveMode.kind]}</p>
            <div className="controller-mapping-editor">
              <ModeFields mode={effectiveMode} parameter={parameter} disabled={busy} onChange={setMode} />
            </div>
            {problem ? <p className="parameter-link-error" role="alert">{problem}</p> : null}
          </fieldset>
        ) : null}
        <button className={`midi-learn-button${learning ? " learning" : ""}`} type="button" disabled={busy} onClick={() => learning ? void stopLearn() : void learn()}>
          {learning ? "Listening… tap to stop" : "Learn next MIDI message"}
        </button>
        <p className="parameter-link-help">Learn only fills this form. RackForge does not change the project or runtime until you press Apply.</p>
      </div>
    </ModalDialog>
  );
}
