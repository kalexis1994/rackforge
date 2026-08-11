import { lazy, Suspense, useEffect, useState, type ReactNode } from "react";
import {
  dispatchCommand,
  dispatchPerformanceEdit,
  requestPluginPreset,
  requestPluginPresets,
} from "./gateway";
import { RfLoader } from "./components/RfLoader";
import {
  addSlotToRack,
  graphFromSlots,
  materializeRackGraph,
  removeSlotFromRack,
} from "./rackGraph";
import type {
  LiveBrowseMode,
  LiveLocation,
  HostPresetSummary,
  PerformanceEdit,
  PerformanceSnapshot,
  PluginInstance,
  RackDefinition,
  RackSlot,
  SessionSnapshot,
  SetlistDefinition,
  SongDefinition,
} from "./types";

const RackGraphEditor = lazy(() => import("./components/RackGraphEditor"));

type ConfigKind = "rack" | "song" | "setlist";

interface LivePageProps {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot | null;
  pending: boolean;
}

const kindLabels: Record<ConfigKind, string> = {
  rack: "Racks",
  song: "Songs",
  setlist: "Setlists",
};

function performanceId(prefix: string) {
  return `${prefix}.${crypto.randomUUID().replaceAll("-", "")}`;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function sameLocation(left: LiveLocation | undefined, right: LiveLocation) {
  return left !== undefined && JSON.stringify(left) === JSON.stringify(right);
}

function describeLocation(
  performance: PerformanceSnapshot,
  location: LiveLocation | undefined,
) {
  if (!location) return { title: "Nothing active", detail: "Choose a LIVE target" };
  const { library } = performance;
  if (location.kind === "rack") {
    const rack = library.racks.find((item) => item.id === location.rack_id);
    return { title: rack?.name ?? "Missing Rack", detail: "Rack" };
  }
  if (location.kind === "song") {
    const song = library.songs.find((item) => item.id === location.song_id);
    const part = song?.parts.find((item) => item.id === location.part_id);
    return {
      title: part?.name ?? "Missing Part",
      detail: `${song?.name ?? "Missing Song"} · Song`,
    };
  }
  const setlist = library.setlists.find(
    (item) => item.id === location.setlist_id,
  );
  const entry = setlist?.entries.find((item) => item.id === location.entry_id);
  const song = performance.library.songs.find(
    (item) => item.id === entry?.song_id,
  );
  const part = song?.parts.find((item) => item.id === location.part_id);
  return {
    title: part?.name ?? "Missing Part",
    detail: `${setlist?.name ?? "Missing Setlist"} · ${song?.name ?? "Missing Song"}`,
  };
}

export function LivePage({ session, performance, pending }: LivePageProps) {
  const [surface, setSurface] = useState<"perform" | "configure">("perform");

  return (
    <section className="live-shell">
      <div className="live-toolbar">
        <div>
          <span className="eyebrow accent">Performance workspace</span>
          <h1>LIVE</h1>
        </div>
        <div className="live-surface-tabs" role="tablist" aria-label="LIVE views">
          <button
            className={surface === "perform" ? "active" : ""}
            onClick={() => setSurface("perform")}
          >
            Perform
          </button>
          <button
            className={surface === "configure" ? "active" : ""}
            onClick={() => setSurface("configure")}
          >
            Configure
          </button>
        </div>
      </div>
      {!performance ? (
        <LiveLoading />
      ) : surface === "perform" ? (
        <PerformanceBrowser session={session} performance={performance} />
      ) : (
        <PerformanceConfig
          session={session}
          performance={performance}
          pending={pending}
        />
      )}
    </section>
  );
}

function LiveLoading() {
  return (
    <div className="live-loading">
      <RfLoader
        label="Live performance"
        detail="Synchronizing with RackForge Core…"
        size="medium"
      />
    </div>
  );
}

function PerformanceBrowser({
  session,
  performance,
}: {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot;
}) {
  const mode = performance.live.mode;
  const active = describeLocation(performance, performance.live.active);
  const activate = (location: LiveLocation) => {
    if (session?.active_mode !== "live") {
      dispatchCommand({ type: "set_active_mode", mode: "live" });
    }
    dispatchCommand({ type: "activate_live_target", location });
  };

  return (
    <div className="live-content">
      <article className="active-performance-card">
        <span className="card-kicker">On stage</span>
        <div>
          <h2>{active.title}</h2>
          <p>{active.detail}</p>
        </div>
        <span className={`live-state${session?.active_mode === "live" ? " online" : ""}`}>
          <i /> {session?.active_mode === "live"
            ? "LIVE ACTIVE"
            : session?.active_mode === "play"
              ? "PLAY MODE"
              : "AUDIO STOPPED"}
        </span>
      </article>

      <div className="live-browser">
        <div className="live-mode-tabs" role="tablist" aria-label="LIVE target type">
          {(["rack", "song", "setlist"] as LiveBrowseMode[]).map((item) => (
            <button
              key={item}
              className={mode === item ? "active" : ""}
              onClick={() =>
                dispatchCommand({ type: "set_live_browse_mode", mode: item })
              }
            >
              {kindLabels[item]}
            </button>
          ))}
        </div>
        <div className="live-target-list">
          {mode === "rack" && (
            <RackTargets performance={performance} activate={activate} />
          )}
          {mode === "song" && (
            <SongTargets performance={performance} activate={activate} />
          )}
          {mode === "setlist" && (
            <SetlistTargets performance={performance} activate={activate} />
          )}
        </div>
      </div>
    </div>
  );
}

function ActivateButton({
  active,
  disabled,
  onClick,
}: {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`activate-button${active ? " active" : ""}`}
      disabled={disabled || active}
      onClick={onClick}
    >
      {active ? "Playing" : "Load"}
    </button>
  );
}

function RackTargets({
  performance,
  activate,
}: {
  performance: PerformanceSnapshot;
  activate: (location: LiveLocation) => void;
}) {
  const racks = performance.library.racks.filter((rack) => rack.enabled);
  if (racks.length === 0) return <LiveEmpty label="No enabled Racks" />;
  return (
    <div className="target-grid">
      {racks.map((rack) => {
        const location: LiveLocation = { kind: "rack", rack_id: rack.id };
        return (
          <article className="target-card" key={rack.id}>
            <span className="target-index">{String(racks.indexOf(rack) + 1).padStart(2, "0")}</span>
            <div>
              <h3>{rack.name}</h3>
              <p>{rack.slots.filter((slot) => slot.enabled).length} active slots</p>
            </div>
            <ActivateButton
              active={sameLocation(performance.live.active, location)}
              onClick={() => activate(location)}
            />
          </article>
        );
      })}
    </div>
  );
}

function SongTargets({
  performance,
  activate,
}: {
  performance: PerformanceSnapshot;
  activate: (location: LiveLocation) => void;
}) {
  const songs = performance.library.songs.filter((song) => song.enabled);
  if (songs.length === 0) return <LiveEmpty label="No enabled Songs" />;
  return (
    <div className="sequence-list">
      {songs.map((song) => (
        <section className="sequence-group" key={song.id}>
          <header>
            <span className="card-kicker">Song</span>
            <h3>{song.name}</h3>
          </header>
          {song.parts.map((part) => {
            const location: LiveLocation = {
              kind: "song",
              song_id: song.id,
              part_id: part.id,
            };
            const rack = performance.library.racks.find(
              (item) => item.id === part.rack_id,
            );
            return (
              <div className="sequence-row" key={part.id}>
                <span>{part.name}</span>
                <small>{rack?.name ?? "Missing Rack"}</small>
                <ActivateButton
                  active={sameLocation(performance.live.active, location)}
                  disabled={!rack?.enabled}
                  onClick={() => activate(location)}
                />
              </div>
            );
          })}
        </section>
      ))}
    </div>
  );
}

function SetlistTargets({
  performance,
  activate,
}: {
  performance: PerformanceSnapshot;
  activate: (location: LiveLocation) => void;
}) {
  const setlists = performance.library.setlists.filter((setlist) => setlist.enabled);
  if (setlists.length === 0) return <LiveEmpty label="No enabled Setlists" />;
  return (
    <div className="sequence-list">
      {setlists.map((setlist) => (
        <section className="sequence-group" key={setlist.id}>
          <header>
            <span className="card-kicker">Setlist</span>
            <h3>{setlist.name}</h3>
          </header>
          {setlist.entries.flatMap((entry, entryIndex) => {
            const song = performance.library.songs.find(
              (item) => item.id === entry.song_id,
            );
            return (song?.parts ?? []).map((part, partIndex) => {
              const location: LiveLocation = {
                kind: "setlist",
                setlist_id: setlist.id,
                entry_id: entry.id,
                part_id: part.id,
              };
              const rack = performance.library.racks.find(
                (item) => item.id === part.rack_id,
              );
              return (
                <div className="sequence-row setlist-row" key={`${entry.id}:${part.id}`}>
                  <span>
                    {entryIndex + 1}.{partIndex + 1} {song?.name ?? "Missing Song"}
                  </span>
                  <small>{part.name}</small>
                  <ActivateButton
                    active={sameLocation(performance.live.active, location)}
                    disabled={!song?.enabled || !rack?.enabled}
                    onClick={() => activate(location)}
                  />
                </div>
              );
            });
          })}
        </section>
      ))}
    </div>
  );
}

function LiveEmpty({ label }: { label: string }) {
  return (
    <div className="live-empty">
      <strong>{label}</strong>
      <p>Create one from the Configure view.</p>
    </div>
  );
}

function PerformanceConfig({
  session,
  performance,
  pending,
}: {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot;
  pending: boolean;
}) {
  const [kind, setKind] = useState<ConfigKind>("rack");
  const [selectedId, setSelectedId] = useState<string | null>(
    performance.library.racks[0]?.id ?? null,
  );
  const [editorDirty, setEditorDirty] = useState(false);
  const items =
    kind === "rack"
      ? performance.library.racks
      : kind === "song"
        ? performance.library.songs
        : performance.library.setlists;

  useEffect(() => {
    if (!editorDirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [editorDirty]);

  const proceed = (action: () => void) => {
    if (
      editorDirty &&
      !window.confirm("Discard the unsaved changes in this editor?")
    )
      return;
    action();
  };
  const selectItem = (id: string) => {
    if (selectedId === id) return;
    proceed(() => setSelectedId(id));
  };
  const changeKind = (next: ConfigKind) => {
    if (next === kind) return;
    proceed(() => {
    setKind(next);
    const collection =
      next === "rack"
        ? performance.library.racks
        : next === "song"
          ? performance.library.songs
          : performance.library.setlists;
    setSelectedId(collection[0]?.id ?? null);
    });
  };

  return (
    <div className="performance-config">
      <aside className="config-library">
        <div className="config-kind-tabs">
          {(["rack", "song", "setlist"] as ConfigKind[]).map((item) => (
            <button
              key={item}
              className={kind === item ? "active" : ""}
              onClick={() => changeKind(item)}
            >
              {kindLabels[item]}
            </button>
          ))}
        </div>
        <button
          className="new-performance-button"
          onClick={() => selectItem("new")}
          disabled={
            (kind === "song" && performance.library.racks.length === 0) ||
            (kind === "setlist" && performance.library.songs.length === 0)
          }
        >
          <span>＋</span> New {kind}
        </button>
        <div className="config-item-list">
          {items.map((item) => (
            <button
              key={item.id}
              className={selectedId === item.id ? "active" : ""}
              onClick={() => selectItem(item.id)}
            >
              <span>{item.name}</span>
              <small>{item.enabled ? "Enabled" : "Disabled"}</small>
            </button>
          ))}
        </div>
      </aside>
      <main className="config-editor">
        {kind === "rack" && (
          <RackEditor
            key={`rack:${selectedId ?? "empty"}`}
            rack={
              selectedId === "new"
                ? newRack(session?.instances ?? [])
                : performance.library.racks.find((item) => item.id === selectedId)
            }
            instances={session?.instances ?? []}
            performance={performance}
            pending={pending}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => setSelectedId(id)}
            onDeleted={() =>
              setSelectedId(performance.library.racks[0]?.id ?? null)
            }
          />
        )}
        {kind === "song" && (
          <SongEditor
            key={`song:${selectedId ?? "empty"}`}
            song={
              selectedId === "new"
                ? newSong(performance)
                : performance.library.songs.find((item) => item.id === selectedId)
            }
            performance={performance}
            pending={pending}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => setSelectedId(id)}
            onDeleted={() =>
              setSelectedId(performance.library.songs[0]?.id ?? null)
            }
          />
        )}
        {kind === "setlist" && (
          <SetlistEditor
            key={`setlist:${selectedId ?? "empty"}`}
            setlist={
              selectedId === "new"
                ? newSetlist(performance)
                : performance.library.setlists.find(
                    (item) => item.id === selectedId,
                  )
            }
            performance={performance}
            pending={pending}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => setSelectedId(id)}
            onDeleted={() =>
              setSelectedId(performance.library.setlists[0]?.id ?? null)
            }
          />
        )}
      </main>
    </div>
  );
}

function defaultSlot(instances: PluginInstance[]): RackSlot {
  const instance = instances[0];
  return {
    id: performanceId("slot"),
    name: instance?.plugin_name ?? "Instrument",
    plugin_id: instance?.plugin_id ?? "org.rackforge.missing",
    enabled: true,
    midi_note_low: 0,
    midi_note_high: 127,
    midi_transpose: 0,
    midi_output: { kind: "none" },
    audio_output_bus: "main",
    level_per_mille: 1000,
    pan_per_mille: 0,
  };
}

function newRack(instances: PluginInstance[]): RackDefinition {
  const slots = [defaultSlot(instances)];
  return {
    schema_version: 1,
    id: performanceId("rack"),
    name: "New Rack",
    enabled: true,
    slots,
    graph: graphFromSlots(slots),
  };
}

function newSong(performance: PerformanceSnapshot): SongDefinition | undefined {
  const rack = performance.library.racks[0];
  if (!rack) return undefined;
  return {
    schema_version: 1,
    id: performanceId("song"),
    name: "New Song",
    enabled: true,
    parts: [{ id: performanceId("part"), name: "Intro", rack_id: rack.id }],
  };
}

function newSetlist(
  performance: PerformanceSnapshot,
): SetlistDefinition | undefined {
  const song = performance.library.songs[0];
  if (!song) return undefined;
  return {
    schema_version: 1,
    id: performanceId("setlist"),
    name: "New Setlist",
    enabled: true,
    entries: [{ id: performanceId("entry"), song_id: song.id }],
  };
}

function EditorEmpty({ children }: { children: ReactNode }) {
  return <div className="editor-empty">{children}</div>;
}

function EditorHeader({
  eyebrow,
  title,
  dirty,
  pending,
  onSave,
  onReset,
  onDelete,
}: {
  eyebrow: string;
  title: string;
  dirty: boolean;
  pending: boolean;
  onSave: () => void;
  onReset: () => void;
  onDelete?: () => void;
}) {
  return (
    <header className="editor-header">
      <div>
        <span className="card-kicker">{eyebrow}</span>
        <h2>{title}</h2>
        <small>{dirty ? "Unsaved changes" : "Saved"}</small>
      </div>
      <div className="editor-actions">
        {onDelete && (
          <button className="danger-button" onClick={onDelete} disabled={pending}>
            Delete
          </button>
        )}
        <button onClick={onReset} disabled={!dirty || pending}>
          Reset
        </button>
        <button className="save-button" onClick={onSave} disabled={!dirty || pending}>
          {pending ? "Saving…" : "Save"}
        </button>
      </div>
    </header>
  );
}

function BasicFields({
  name,
  enabled,
  onName,
  onEnabled,
}: {
  name: string;
  enabled: boolean;
  onName: (name: string) => void;
  onEnabled: (enabled: boolean) => void;
}) {
  return (
    <div className="form-grid basic-fields">
      <label>
        <span>Name</span>
        <input
          value={name}
          maxLength={64}
          onChange={(event) => onName(event.target.value)}
        />
      </label>
      <label className="toggle-field">
        <span>Available in LIVE</span>
        <input
          type="checkbox"
          checked={enabled}
          onChange={(event) => onEnabled(event.target.checked)}
        />
        <i />
      </label>
    </div>
  );
}

function validationName(name: string) {
  return name.trim().length === 0
    ? "Name is required."
    : [...name].length > 64
      ? "Name cannot exceed 64 characters."
      : null;
}

function dispatchEdit(
  expectedRevision: string,
  edit: PerformanceEdit,
) {
  return dispatchPerformanceEdit(expectedRevision, edit);
}

function RackEditor({
  rack,
  instances,
  performance,
  pending,
  onDirtyChange,
  onSaved,
  onDeleted,
}: {
  rack?: RackDefinition;
  instances: PluginInstance[];
  performance: PerformanceSnapshot;
  pending: boolean;
  onDirtyChange: (dirty: boolean) => void;
  onSaved: (id: string) => void;
  onDeleted: () => void;
}) {
  const original = rack;
  const [draft, setDraft] = useState(() =>
    rack ? clone(materializeRackGraph(rack)) : undefined,
  );
  const [baseRevision, setBaseRevision] = useState(performance.revision);
  const [error, setError] = useState<string | null>(null);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(original);
  const isNew = !!draft && !performance.library.racks.some((item) => item.id === draft.id);
  useEffect(() => {
    onDirtyChange(dirty || isNew);
    return () => onDirtyChange(false);
  }, [dirty, isNew, onDirtyChange]);
  if (!draft)
    return <EditorEmpty>Select a Rack or create a new one.</EditorEmpty>;

  const updateSlot = (index: number, next: RackSlot) => {
    const slots = [...draft.slots];
    slots[index] = next;
    setDraft({ ...draft, slots });
  };
  const validate = () => {
    const nameError = validationName(draft.name);
    if (nameError) return nameError;
    if (draft.slots.length === 0) return "A Rack needs at least one Slot.";
    if (!draft.slots.some((slot) => slot.enabled))
      return "A Rack needs at least one enabled Slot.";
    for (const slot of draft.slots) {
      if (validationName(slot.name)) return "Every Slot needs a valid name.";
      if (!instances.some((instance) => instance.plugin_id === slot.plugin_id))
        return `${slot.name} needs an available plugin.`;
    }
    return null;
  };
  const save = async () => {
    const nextError = validate();
    setError(nextError);
    if (nextError) return;
    try {
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "put_rack",
        rack: draft,
      });
      const saved = snapshot.library.racks.find((item) => item.id === draft.id);
      if (saved) setDraft(clone(saved));
      setBaseRevision(snapshot.revision);
      onSaved(draft.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save Rack.");
    }
  };
  const usedBy = [
    ...performance.library.songs
      .filter((song) => song.parts.some((part) => part.rack_id === draft.id))
      .map((song) => song.name),
    ...(performance.live.active_rack_id === draft.id ? ["the active LIVE target"] : []),
  ];
  const remove = async () => {
    if (usedBy.length) {
      setError(`Cannot delete this Rack; it is used by ${usedBy.join(", ")}.`);
      return;
    }
    if (
      !window.confirm(`Delete Rack “${draft.name}”? This cannot be undone.`)
    )
      return;
    try {
      await dispatchEdit(baseRevision, {
        kind: "delete_rack",
        rack_id: draft.id,
      });
      onDeleted();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not delete Rack.");
    }
  };

  return (
    <form className="performance-form" onSubmit={(event) => event.preventDefault()}>
      <EditorHeader
        eyebrow={isNew ? "New Rack" : "Rack configuration"}
        title={draft.name}
        dirty={dirty || isNew}
        pending={pending}
        onSave={save}
        onReset={() => {
          setDraft(
            original ? clone(materializeRackGraph(original)) : newRack(instances),
          );
          setBaseRevision(performance.revision);
          setError(null);
        }}
        onDelete={isNew ? undefined : remove}
      />
      {error && <div className="form-error">{error}</div>}
      <BasicFields
        name={draft.name}
        enabled={draft.enabled}
        onName={(name) => setDraft({ ...draft, name })}
        onEnabled={(enabled) => setDraft({ ...draft, enabled })}
      />
      <EditorSection
        title="Rack graph"
        detail="Route instruments and child Racks. Positions and labels are portable; the viewport stays local to this device."
        action={null}
      >
        <Suspense fallback={<div className="rack-graph-loading">Loading graph editor…</div>}>
          <RackGraphEditor
            rack={draft}
            racks={performance.library.racks}
            onChange={setDraft}
          />
        </Suspense>
      </EditorSection>
      <EditorSection
        title="Instrument settings"
        detail="Configure plugin state, MIDI filters and mix for each instrument node."
        action={
          <button
            onClick={() => setDraft(addSlotToRack(draft, defaultSlot(instances)))}
            disabled={draft.slots.length >= 32 || instances.length === 0}
          >
            ＋ Add Slot
          </button>
        }
      >
        {draft.slots.map((slot, index) => (
          <SlotEditor
            key={`${slot.id}:${slot.plugin_id}`}
            slot={slot}
            index={index}
            total={draft.slots.length}
            instances={instances}
            onChange={(next) => updateSlot(index, next)}
            onMove={(direction) => {
              const target = index + direction;
              if (target < 0 || target >= draft.slots.length) return;
              const slots = [...draft.slots];
              [slots[index], slots[target]] = [slots[target], slots[index]];
              setDraft({ ...draft, slots });
            }}
            onRemove={() =>
              setDraft(removeSlotFromRack(draft, slot.id))
            }
          />
        ))}
      </EditorSection>
    </form>
  );
}

function SlotEditor({
  slot,
  index,
  total,
  instances,
  onChange,
  onMove,
  onRemove,
}: {
  slot: RackSlot;
  index: number;
  total: number;
  instances: PluginInstance[];
  onChange: (slot: RackSlot) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
}) {
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [presetId, setPresetId] = useState("");
  const [presetBusy, setPresetBusy] = useState(false);
  const [presetError, setPresetError] = useState<string | null>(null);
  const pluginAvailable = instances.some((item) => item.plugin_id === slot.plugin_id);
  useEffect(() => {
    if (!pluginAvailable) return;
    let active = true;
    requestPluginPresets(slot.plugin_id)
      .then((items) => {
        if (!active) return;
        setPresets(items);
        setPresetError(null);
      })
      .catch((error: Error) => {
        if (active) setPresetError(error.message);
      });
    return () => {
      active = false;
    };
  }, [slot.plugin_id, pluginAvailable]);
  const visiblePresets = pluginAvailable ? presets : [];
  const visiblePresetError = pluginAvailable ? presetError : null;
  const setPlugin = (pluginId: string) => {
    onChange({
      ...slot,
      plugin_id: pluginId,
      state: undefined,
      legacy_program_id: undefined,
    });
  };
  const loadPreset = () => {
    if (!presetId) return;
    setPresetBusy(true);
    setPresetError(null);
    requestPluginPreset(slot.plugin_id, presetId)
      .then((preset) => {
        onChange({
          ...slot,
          state: preset.state,
          legacy_program_id: undefined,
        });
      })
      .catch((error: Error) => setPresetError(error.message))
      .finally(() => setPresetBusy(false));
  };
  return (
    <article className={`slot-editor${slot.enabled ? "" : " disabled"}`}>
      <header>
        <span className="slot-number">{String(index + 1).padStart(2, "0")}</span>
        <strong>{slot.name || "Unnamed Slot"}</strong>
        <div className="reorder-controls">
          <button aria-label="Move Slot up" disabled={index === 0} onClick={() => onMove(-1)}>↑</button>
          <button aria-label="Move Slot down" disabled={index === total - 1} onClick={() => onMove(1)}>↓</button>
          <button aria-label="Remove Slot" disabled={total === 1} onClick={onRemove}>×</button>
        </div>
      </header>
      <div className="form-grid slot-fields">
        <label>
          <span>Slot name</span>
          <input value={slot.name} maxLength={64} onChange={(event) => onChange({ ...slot, name: event.target.value })} />
        </label>
        <label>
          <span>Plugin</span>
          <select value={slot.plugin_id} onChange={(event) => setPlugin(event.target.value)}>
            {instances.map((item) => <option value={item.plugin_id} key={item.instance_id}>{item.plugin_name}</option>)}
          </select>
        </label>
        {pluginAvailable && (
          <div className="slot-preset-field">
            <span>Load preset</span>
            <div>
              <select value={presetId} onChange={(event) => setPresetId(event.target.value)}>
                <option value="">{presets.length ? "Choose a preset" : "No saved presets"}</option>
                {visiblePresets.map((preset) => <option value={preset.id} key={preset.id}>{preset.name}</option>)}
              </select>
              <button type="button" disabled={!presetId || presetBusy} onClick={loadPreset}>{presetBusy ? "Loading…" : "Load"}</button>
            </div>
            <small>{slot.state ? "State copied into this Slot · independent from its preset" : "Default plugin state"}</small>
            {visiblePresetError && <small className="field-error">{visiblePresetError}</small>}
          </div>
        )}
        <label>
          <span>MIDI input</span>
          <select
            value={slot.midi_input_channel ?? "omni"}
            onChange={(event) => onChange({ ...slot, midi_input_channel: event.target.value === "omni" ? undefined : Number(event.target.value) })}
          >
            <option value="omni">OMNI</option>
            {Array.from({ length: 16 }, (_, item) => item + 1).map((channel) => <option value={channel} key={channel}>Channel {channel}</option>)}
          </select>
        </label>
        <label>
          <span>Low key</span>
          <input
            type="number"
            min="0"
            max={slot.midi_note_high}
            value={slot.midi_note_low}
            onChange={(event) => onChange({ ...slot, midi_note_low: Math.min(slot.midi_note_high, Number(event.target.value)) })}
          />
        </label>
        <label>
          <span>High key</span>
          <input
            type="number"
            min={slot.midi_note_low}
            max="127"
            value={slot.midi_note_high}
            onChange={(event) => onChange({ ...slot, midi_note_high: Math.max(slot.midi_note_low, Number(event.target.value)) })}
          />
        </label>
        <label>
          <span>Octave</span>
          <select
            value={slot.midi_transpose / 12}
            onChange={(event) => onChange({ ...slot, midi_transpose: Number(event.target.value) * 12 })}
          >
            {Array.from({ length: 9 }, (_, item) => item - 4).map((octave) => (
              <option value={octave} key={octave}>{octave === 0 ? "Original" : `${octave > 0 ? "+" : ""}${octave}`}</option>
            ))}
          </select>
        </label>
        <label className="range-field">
          <span>Level <output>{Math.round(slot.level_per_mille / 10)}%</output></span>
          <input type="range" min="0" max="1000" step="10" value={slot.level_per_mille} onChange={(event) => onChange({ ...slot, level_per_mille: Number(event.target.value) })} />
        </label>
        <label className="range-field">
          <span>Pan <output>{slot.pan_per_mille === 0 ? "C" : `${slot.pan_per_mille < 0 ? "L" : "R"}${Math.round(Math.abs(slot.pan_per_mille) / 10)}`}</output></span>
          <input type="range" min="-1000" max="1000" step="10" value={slot.pan_per_mille} onDoubleClick={() => onChange({ ...slot, pan_per_mille: 0 })} onChange={(event) => onChange({ ...slot, pan_per_mille: Number(event.target.value) })} />
        </label>
        <label className="toggle-field compact-toggle">
          <span>Slot enabled</span>
          <input type="checkbox" checked={slot.enabled} onChange={(event) => onChange({ ...slot, enabled: event.target.checked })} />
          <i />
        </label>
        <div className="readonly-field"><span>Audio output</span><strong>Main</strong></div>
      </div>
    </article>
  );
}

function SongEditor({
  song,
  performance,
  pending,
  onDirtyChange,
  onSaved,
  onDeleted,
}: {
  song?: SongDefinition;
  performance: PerformanceSnapshot;
  pending: boolean;
  onDirtyChange: (dirty: boolean) => void;
  onSaved: (id: string) => void;
  onDeleted: () => void;
}) {
  const original = song;
  const [draft, setDraft] = useState(() => (song ? clone(song) : undefined));
  const [baseRevision, setBaseRevision] = useState(performance.revision);
  const [error, setError] = useState<string | null>(null);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(original);
  const isNew = !!draft && !performance.library.songs.some((item) => item.id === draft.id);
  useEffect(() => {
    onDirtyChange(dirty || isNew);
    return () => onDirtyChange(false);
  }, [dirty, isNew, onDirtyChange]);
  if (!draft) return <EditorEmpty>Create a Rack before adding a Song.</EditorEmpty>;
  const save = async () => {
    const nextError = validationName(draft.name) ??
      (draft.parts.length === 0 ? "A Song needs at least one Part." : null) ??
      (draft.parts.find((part) => validationName(part.name)) ? "Every Part needs a valid name." : null);
    setError(nextError);
    if (nextError) return;
    try {
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "put_song",
        song: draft,
      });
      const saved = snapshot.library.songs.find((item) => item.id === draft.id);
      if (saved) setDraft(clone(saved));
      setBaseRevision(snapshot.revision);
      onSaved(draft.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save Song.");
    }
  };
  const usedBy = performance.library.setlists
    .filter((setlist) => setlist.entries.some((entry) => entry.song_id === draft.id))
    .map((setlist) => setlist.name);
  const remove = async () => {
    if (usedBy.length) {
      setError(`Cannot delete this Song; it is used by ${usedBy.join(", ")}.`);
      return;
    }
    if (!window.confirm(`Delete Song “${draft.name}”? This cannot be undone.`)) return;
    try {
      await dispatchEdit(baseRevision, {
        kind: "delete_song",
        song_id: draft.id,
      });
      onDeleted();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not delete Song.");
    }
  };
  return (
    <form className="performance-form" onSubmit={(event) => event.preventDefault()}>
      <EditorHeader eyebrow={isNew ? "New Song" : "Song configuration"} title={draft.name} dirty={dirty || isNew} pending={pending} onSave={save} onReset={() => { setDraft(original ? clone(original) : newSong(performance)); setBaseRevision(performance.revision); setError(null); }} onDelete={isNew ? undefined : remove} />
      {error && <div className="form-error">{error}</div>}
      <BasicFields name={draft.name} enabled={draft.enabled} onName={(name) => setDraft({ ...draft, name })} onEnabled={(enabled) => setDraft({ ...draft, enabled })} />
      <EditorSection title="Parts" detail="Each Part recalls one Rack. Their order becomes the Song navigation order." action={<button disabled={draft.parts.length >= 64} onClick={() => setDraft({ ...draft, parts: [...draft.parts, { id: performanceId("part"), name: `Part ${draft.parts.length + 1}`, rack_id: performance.library.racks[0].id }] })}>＋ Add Part</button>}>
        {draft.parts.map((part, index) => (
          <SequenceEditorRow key={part.id} index={index} total={draft.parts.length} title={part.name} onTitle={(name) => { const parts = [...draft.parts]; parts[index] = { ...part, name }; setDraft({ ...draft, parts }); }} onMove={(direction) => { const target = index + direction; if (target < 0 || target >= draft.parts.length) return; const parts = [...draft.parts]; [parts[index], parts[target]] = [parts[target], parts[index]]; setDraft({ ...draft, parts }); }} onRemove={() => setDraft({ ...draft, parts: draft.parts.filter((item) => item.id !== part.id) })}>
            <label><span>Rack</span><select value={part.rack_id} onChange={(event) => { const parts = [...draft.parts]; parts[index] = { ...part, rack_id: event.target.value }; setDraft({ ...draft, parts }); }}>{performance.library.racks.map((rack) => <option value={rack.id} key={rack.id}>{rack.name}</option>)}</select></label>
          </SequenceEditorRow>
        ))}
      </EditorSection>
    </form>
  );
}

function SetlistEditor({
  setlist,
  performance,
  pending,
  onDirtyChange,
  onSaved,
  onDeleted,
}: {
  setlist?: SetlistDefinition;
  performance: PerformanceSnapshot;
  pending: boolean;
  onDirtyChange: (dirty: boolean) => void;
  onSaved: (id: string) => void;
  onDeleted: () => void;
}) {
  const original = setlist;
  const [draft, setDraft] = useState(() => (setlist ? clone(setlist) : undefined));
  const [baseRevision, setBaseRevision] = useState(performance.revision);
  const [error, setError] = useState<string | null>(null);
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(original);
  const isNew = !!draft && !performance.library.setlists.some((item) => item.id === draft.id);
  useEffect(() => {
    onDirtyChange(dirty || isNew);
    return () => onDirtyChange(false);
  }, [dirty, isNew, onDirtyChange]);
  if (!draft) return <EditorEmpty>Create a Song before adding a Setlist.</EditorEmpty>;
  const save = async () => {
    const nextError = validationName(draft.name) ?? (draft.entries.length === 0 ? "A Setlist needs at least one Song." : null);
    setError(nextError);
    if (nextError) return;
    try {
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "put_setlist",
        setlist: draft,
      });
      const saved = snapshot.library.setlists.find((item) => item.id === draft.id);
      if (saved) setDraft(clone(saved));
      setBaseRevision(snapshot.revision);
      onSaved(draft.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save Setlist.");
    }
  };
  const remove = async () => {
    if (!window.confirm(`Delete Setlist “${draft.name}”? This cannot be undone.`)) return;
    try {
      await dispatchEdit(baseRevision, {
        kind: "delete_setlist",
        setlist_id: draft.id,
      });
      onDeleted();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not delete Setlist.");
    }
  };
  return (
    <form className="performance-form" onSubmit={(event) => event.preventDefault()}>
      <EditorHeader eyebrow={isNew ? "New Setlist" : "Setlist configuration"} title={draft.name} dirty={dirty || isNew} pending={pending} onSave={save} onReset={() => { setDraft(original ? clone(original) : newSetlist(performance)); setBaseRevision(performance.revision); setError(null); }} onDelete={isNew ? undefined : remove} />
      {error && <div className="form-error">{error}</div>}
      <BasicFields name={draft.name} enabled={draft.enabled} onName={(name) => setDraft({ ...draft, name })} onEnabled={(enabled) => setDraft({ ...draft, enabled })} />
      <EditorSection title="Running order" detail="Songs may appear more than once. The order here is the exact show order." action={<button disabled={draft.entries.length >= 256} onClick={() => setDraft({ ...draft, entries: [...draft.entries, { id: performanceId("entry"), song_id: performance.library.songs[0].id }] })}>＋ Add Song</button>}>
        {draft.entries.map((entry, index) => {
          const song = performance.library.songs.find((item) => item.id === entry.song_id);
          return <SequenceEditorRow key={entry.id} index={index} total={draft.entries.length} title={song?.name ?? "Missing Song"} fixedTitle onTitle={() => undefined} onMove={(direction) => { const target = index + direction; if (target < 0 || target >= draft.entries.length) return; const entries = [...draft.entries]; [entries[index], entries[target]] = [entries[target], entries[index]]; setDraft({ ...draft, entries }); }} onRemove={() => setDraft({ ...draft, entries: draft.entries.filter((item) => item.id !== entry.id) })}>
            <label><span>Song</span><select value={entry.song_id} onChange={(event) => { const entries = [...draft.entries]; entries[index] = { ...entry, song_id: event.target.value }; setDraft({ ...draft, entries }); }}>{performance.library.songs.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></label>
          </SequenceEditorRow>;
        })}
      </EditorSection>
    </form>
  );
}

function EditorSection({
  title,
  detail,
  action,
  children,
}: {
  title: string;
  detail: string;
  action: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="editor-section">
      <header><div><h3>{title}</h3><p>{detail}</p></div>{action}</header>
      <div className="editor-section-content">{children}</div>
    </section>
  );
}

function SequenceEditorRow({
  index,
  total,
  title,
  fixedTitle = false,
  onTitle,
  onMove,
  onRemove,
  children,
}: {
  index: number;
  total: number;
  title: string;
  fixedTitle?: boolean;
  onTitle: (name: string) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
  children: ReactNode;
}) {
  return (
    <article className="sequence-editor-row">
      <span className="slot-number">{String(index + 1).padStart(2, "0")}</span>
      {fixedTitle ? <strong>{title}</strong> : <label><span>Part name</span><input value={title} maxLength={64} onChange={(event) => onTitle(event.target.value)} /></label>}
      {children}
      <div className="reorder-controls">
        <button disabled={index === 0} onClick={() => onMove(-1)}>↑</button>
        <button disabled={index === total - 1} onClick={() => onMove(1)}>↓</button>
        <button disabled={total === 1} onClick={onRemove}>×</button>
      </div>
    </article>
  );
}
