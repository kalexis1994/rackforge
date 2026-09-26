import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import {
  materializePluginState,
  requestPluginCatalog,
  requestPluginPreset,
  requestPluginPresets,
} from "../gateway";
import { AsyncActionLabel, AsyncSpinner } from "./AsyncSpinner";
import { isRackSlotStub } from "../rackPluginSelection";
import { useCanvasModal } from "../hooks/useCanvasModal";
import type {
  HostPresetSummary,
  PluginInstance,
  PluginStateReference,
  RackSlot,
} from "../types";

/** The plugin's kind as the colour code names it (design/tokens.css). */
export type RackPluginEditorKind = "instrument" | "effect" | "midi-processor";

const KIND_LABEL: Record<RackPluginEditorKind, string> = {
  instrument: "Instrument",
  effect: "Effect",
  "midi-processor": "MIDI processor",
};

type PresetList =
  | { status: "loading" }
  | { status: "ready"; items: HostPresetSummary[] }
  | { status: "failed"; message: string };

interface RackPluginEditorProps {
  slot: RackSlot;
  /** The running plugin; without one there is nothing to draw, and it says so. */
  instance?: PluginInstance;
  kind: RackPluginEditorKind;
  art?: { iconUrl?: string; version?: string };
  onChange: (slot: RackSlot) => void;
  onClose: () => void;
  renderSurface: (options: {
    instance: PluginInstance;
    state?: PluginStateReference;
    onStateChange: (state: PluginStateReference) => void;
    onSelectSound: (soundId: string) => Promise<unknown>;
    parameterLinkInstanceId: string;
  }) => ReactNode;
}

type SurfaceOptions = Parameters<RackPluginEditorProps["renderSurface"]>[0];

/**
 * Draws the plugin's surface through the caller's renderer as a component of
 * its own, so the editor hands its callbacks over as props rather than as
 * arguments of a call made while the editor renders.
 */
function PluginSurface({
  render,
  options,
}: {
  render: RackPluginEditorProps["renderSurface"];
  options: SurfaceOptions;
}) {
  return <>{render(options)}</>;
}

/**
 * A plugin in a Rack, edited: the plugin's own surface over the whole
 * editor, edged in its kind's colour, with its presets in theirs. It is a
 * modal -- the graph behind it is inert until it is closed -- so what is
 * edited is never a node the pointer can move or delete meanwhile.
 */
export function RackPluginEditor({
  slot,
  instance,
  kind,
  art,
  onChange,
  onClose,
  renderSurface,
}: RackPluginEditorProps) {
  const [presetsAttempt, setPresetsAttempt] = useState(0);
  // The list is the answer to one question -- this plugin, this attempt --
  // and until that question is answered the list is loading.
  const presetsQuestion = `${slot.plugin_id}\n${presetsAttempt}`;
  const [presetsAnswer, setPresetsAnswer] = useState<{ question: string; list: PresetList } | null>(
    null,
  );
  const presets: PresetList =
    presetsAnswer?.question === presetsQuestion ? presetsAnswer.list : { status: "loading" };
  const [presetId, setPresetId] = useState("");
  const [loadedPresetId, setLoadedPresetId] = useState<string>();
  const [busy, setBusy] = useState<"preset" | "sound" | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The programs of a plugin the host is not running. A host that runs one
  // plugin at a time (Android) carries only that one's programs in the
  // session; a Slot holding another asks for its catalog, and the surface is
  // drawn once it has it, so the plugin boots with its programs listed.
  const needsCatalog = Boolean(
    instance && isRackSlotStub(instance) && instance.sounds.length === 0,
  );
  const [catalog, setCatalog] = useState<
    | { pluginId: string; sounds: PluginInstance["sounds"]; banks: PluginInstance["banks"] }
    | { pluginId: string; failed: string }
    | null
  >(null);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const { sectionRef, closeRef, onKeyDown } = useCanvasModal(onClose);
  // Answers that arrive after the editor is closed are dropped; the slot a
  // late answer would be applied to is the one it was asked for.
  const mounted = useRef(true);
  const latestSlot = useRef(slot);
  useLayoutEffect(() => {
    latestSlot.current = slot;
  });
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    const question = presetsQuestion;
    requestPluginPresets(slot.plugin_id)
      .then((items) => {
        if (active) setPresetsAnswer({ question, list: { status: "ready", items } });
      })
      .catch((reason: unknown) => {
        if (active) {
          setPresetsAnswer({
            question,
            list: {
              status: "failed",
              message: reason instanceof Error ? reason.message : "The presets could not be listed.",
            },
          });
        }
      });
    return () => {
      active = false;
    };
  }, [slot.plugin_id, presetsQuestion]);

  useEffect(() => {
    if (!needsCatalog) return;
    let active = true;
    const pluginId = slot.plugin_id;
    requestPluginCatalog(pluginId)
      .then(({ sounds, banks }) => {
        if (active) setCatalog({ pluginId, sounds, banks });
      })
      .catch((reason: unknown) => {
        if (active) {
          setCatalog({
            pluginId,
            failed: reason instanceof Error ? reason.message : "The programs could not be listed.",
          });
        }
      });
    return () => {
      active = false;
    };
  }, [needsCatalog, slot.plugin_id, catalogAttempt]);

  const selectSound = useCallback(async (soundId: string) => {
    setBusy("sound");
    setError(null);
    try {
      const state = await materializePluginState(slot.plugin_id, soundId);
      if (mounted.current) {
        onChange({ ...latestSlot.current, state, legacy_program_id: undefined });
        setLoadedPresetId(undefined);
      }
      return { sound_id: soundId, isolated: true, state };
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : "This sound could not be applied.";
      if (mounted.current) setError(message);
      throw new Error(message);
    } finally {
      if (mounted.current) setBusy(null);
    }
  }, [onChange, slot.plugin_id]);

  const updateState = useCallback((state: PluginStateReference) => {
    onChange({ ...latestSlot.current, state, legacy_program_id: undefined });
  }, [onChange]);

  const loadPreset = useCallback(async () => {
    if (!presetId || busy) return;
    setBusy("preset");
    setError(null);
    try {
      const preset = await requestPluginPreset(slot.plugin_id, presetId);
      if (!mounted.current) return;
      onChange({ ...latestSlot.current, state: preset.state, legacy_program_id: undefined });
      setLoadedPresetId(presetId);
    } catch (reason) {
      if (mounted.current) {
        setError(reason instanceof Error ? reason.message : "This preset could not be loaded.");
      }
    } finally {
      if (mounted.current) setBusy(null);
    }
  }, [busy, onChange, presetId, slot.plugin_id]);

  const slotCatalog = needsCatalog && catalog?.pluginId === slot.plugin_id ? catalog : null;
  const catalogFailed = slotCatalog && "failed" in slotCatalog ? slotCatalog.failed : null;
  const catalogPending = needsCatalog && slotCatalog === null;
  const editorInstance: PluginInstance | undefined = instance ? {
    ...instance,
    ...(slotCatalog && "sounds" in slotCatalog
      ? { sounds: slotCatalog.sounds, banks: slotCatalog.banks }
      : {}),
    selected_sound_id: slot.state?.selected_sound_id ?? instance.selected_sound_id,
  } : undefined;
  const kindLabel = KIND_LABEL[kind];
  const titleId = `rack-plugin-editor-title-${slot.id}`;
  const items = presets.status === "ready" ? presets.items : [];

  return (
    <>
      <div className="rack-plugin-editor-scrim" aria-hidden="true" />
      <section
        ref={sectionRef}
        className={`rack-plugin-editor kind-${kind}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={onKeyDown}
      >
        <header className="rack-plugin-editor-header">
          <div className="rack-plugin-editor-title">
            {art?.iconUrl ? (
              <img className="rack-plugin-editor-icon" src={art.iconUrl} alt="" draggable={false} />
            ) : null}
            <div>
              <span className="rack-plugin-editor-kind">{kindLabel}</span>
              <strong id={titleId}>
                {slot.name}
                {art?.version ? <small>{art.version}</small> : null}
              </strong>
            </div>
          </div>
          <div
            className={`rack-plugin-editor-presets is-${presets.status}`}
            role="group"
            aria-label="Presets"
          >
            <label htmlFor={`${titleId}-preset`}>Presets</label>
            {presets.status === "failed" ? (
              <p className="rack-plugin-editor-presets-note" role="alert">
                <span title={presets.message}>Presets unavailable.</span>
                <button type="button" onClick={() => setPresetsAttempt((attempt) => attempt + 1)}>
                  Retry
                </button>
              </p>
            ) : (
              <>
                <select
                  id={`${titleId}-preset`}
                  value={presetId}
                  disabled={busy !== null || presets.status === "loading" || items.length === 0}
                  onChange={(event) => setPresetId(event.target.value)}
                >
                  <option value="">
                    {presets.status === "loading"
                      ? "Loading presets…"
                      : items.length
                        ? "Choose a preset"
                        : "No saved presets"}
                  </option>
                  {items.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.id === loadedPresetId ? `${preset.name} (loaded)` : preset.name}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="rack-plugin-editor-load"
                  disabled={!presetId || busy !== null}
                  onClick={() => void loadPreset()}
                >
                  <AsyncActionLabel active={busy === "preset"} activeLabel="Loading…">Load</AsyncActionLabel>
                </button>
              </>
            )}
          </div>
          <button
            ref={closeRef}
            type="button"
            className="rack-plugin-editor-close"
            aria-label={`Close the ${kindLabel.toLowerCase()} editor`}
            title="Close (Esc)"
            onClick={onClose}
          >
            <X aria-hidden="true" />
          </button>
        </header>
        {error ? (
          <div className="rack-plugin-editor-error" role="alert">
            <span>{error}</span>
            <button type="button" onClick={() => setError(null)} aria-label="Dismiss">
              <X aria-hidden="true" />
            </button>
          </div>
        ) : null}
        {catalogFailed ? (
          <div className="rack-plugin-editor-error" role="alert">
            <span>The programs could not be listed: {catalogFailed}</span>
            <button type="button" onClick={() => {
              setCatalog(null);
              setCatalogAttempt((attempt) => attempt + 1);
            }}>
              Retry
            </button>
          </div>
        ) : null}
        <div className="rack-plugin-editor-surface" aria-busy={busy !== null || catalogPending}>
          {catalogPending ? (
            <div className="rack-plugin-editor-busy" role="status" aria-live="polite">
              <AsyncSpinner label="Reading the programs…" size="medium" />
              <span>Reading the programs…</span>
            </div>
          ) : editorInstance ? (
            <PluginSurface
              render={renderSurface}
              options={{
                instance: editorInstance,
                state: slot.state,
                onStateChange: updateState,
                onSelectSound: selectSound,
                parameterLinkInstanceId: slot.id,
              }}
            />
          ) : (
            <p className="rack-plugin-editor-missing">
              This plugin is not running, so its editor cannot be shown. Check that it is
              installed in the Plugin Manager.
            </p>
          )}
          {busy ? (
            <div className="rack-plugin-editor-busy" role="status" aria-live="polite">
              <AsyncSpinner label="Applying the new state…" size="medium" />
              <span>{busy === "preset" ? "Loading the preset…" : "Applying the sound…"}</span>
            </div>
          ) : null}
        </div>
        <footer className="rack-plugin-editor-footer">
          <span>
            {slot.state
              ? `State v${slot.state.state_version} · plugin ${slot.state.plugin_version}`
              : "Plugin default state"}
          </span>
          <span>Esc closes</span>
        </footer>
      </section>
    </>
  );
}
