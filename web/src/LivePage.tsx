import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  dispatchCommand,
  dispatchCommandAwait,
  dispatchPerformanceEdit,
  requestPluginPreset,
  requestPluginPresets,
} from "./gateway";
import { RfLoader } from "./components/RfLoader";
import { AsyncActionLabel, AsyncSpinner } from "./components/AsyncSpinner";
import { PerformanceInfoBar } from "./components/PerformanceInfoBar";
import { scopedId } from "./ids";
import { isNativeHost } from "./host";
import {
  addSlotToRack,
  graphFromSlots,
  materializeRackGraph,
  normalizeRackGraphGeometry,
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
  RackGraphPosition,
  RackSlot,
  SessionSnapshot,
  SetlistDefinition,
  SongDefinition,
} from "./types";

const RackGraphEditor = lazy(() => import("./components/RackGraphEditor"));

type ConfigKind = "rack" | "song" | "setlist";

interface PendingPerformanceDelete {
  kind: ConfigKind;
  id: string;
  name: string;
}

interface RackCascadePlan {
  dependentRacks: RackDefinition[];
  songs: SongDefinition[];
  setlists: Array<{
    current: SetlistDefinition;
    next: SetlistDefinition | null;
    removedEntries: number;
  }>;
  rackDeleteOrder: string[];
}

interface LivePageProps {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot | null;
  pending: boolean;
  surface: "perform" | "configure";
  onSurfaceChange: (surface: "perform" | "configure") => void;
  onWorkspaceChange: (workspace: { kind: "rack"; name: string } | null) => void;
  renderPluginSurface: (options: {
    instance: PluginInstance;
    onSelectSound: (soundId: string) => Promise<unknown>;
  }) => ReactNode;
}

const kindLabels: Record<ConfigKind, string> = {
  rack: "Racks",
  song: "Songs",
  setlist: "Setlists",
};

function performanceId(prefix: string) {
  return scopedId(prefix);
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function rackCascadePlan(
  performance: PerformanceSnapshot,
  targetRackId: string,
): RackCascadePlan {
  const rackIds = new Set([targetRackId]);
  let expanded = true;
  while (expanded) {
    expanded = false;
    for (const rack of performance.library.racks) {
      if (rackIds.has(rack.id)) continue;
      const referencesDeletedRack = materializeRackGraph(rack).graph!.nodes.some(
        (node) => node.kind.kind === "rack" && rackIds.has(node.kind.rack_id),
      );
      if (referencesDeletedRack) {
        rackIds.add(rack.id);
        expanded = true;
      }
    }
  }

  const racks = performance.library.racks.filter((rack) => rackIds.has(rack.id));
  const songs = performance.library.songs.filter((song) =>
    song.parts.some((part) => rackIds.has(part.rack_id)),
  );
  const songIds = new Set(songs.map((song) => song.id));
  const setlists = performance.library.setlists.flatMap((setlist) => {
    const entries = setlist.entries.filter((entry) => !songIds.has(entry.song_id));
    const removedEntries = setlist.entries.length - entries.length;
    if (removedEntries === 0) return [];
    return [{
      current: setlist,
      next: entries.length > 0 ? { ...setlist, entries } : null,
      removedEntries,
    }];
  });

  const remaining = new Set(racks.map((rack) => rack.id));
  const rackDeleteOrder: string[] = [];
  while (remaining.size > 0) {
    const referenced = new Set<string>();
    for (const rack of racks) {
      if (!remaining.has(rack.id)) continue;
      for (const node of materializeRackGraph(rack).graph!.nodes) {
        if (node.kind.kind === "rack" && remaining.has(node.kind.rack_id)) {
          referenced.add(node.kind.rack_id);
        }
      }
    }
    const outermost = [...remaining]
      .filter((rackId) => !referenced.has(rackId))
      .sort();
    const next = outermost.length > 0 ? outermost : [...remaining].sort();
    for (const rackId of next) {
      remaining.delete(rackId);
      rackDeleteOrder.push(rackId);
    }
  }

  return {
    dependentRacks: racks.filter((rack) => rack.id !== targetRackId),
    songs,
    setlists,
    rackDeleteOrder,
  };
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

export function LivePage({
  session,
  performance,
  pending,
  surface,
  onSurfaceChange,
  onWorkspaceChange,
  renderPluginSurface,
}: LivePageProps) {
  const [workspace, setWorkspace] = useState<{ kind: "rack"; name: string } | null>(null);
  const handleWorkspaceChange = useCallback(
    (next: { kind: "rack"; name: string } | null) => {
      setWorkspace(next);
      onWorkspaceChange(next);
    },
    [onWorkspaceChange],
  );
  const active = performance
    ? describeLocation(performance, performance.live.active)
    : { title: "Synchronizing", detail: "RackForge Core" };
  const liveContext = performance
    ? kindLabels[performance.live.mode]
    : "Performance";
  return (
    <section className={`live-shell live-${surface}-surface`}>
      <PerformanceInfoBar
        className="live-performance-bar"
        left={{ label: "Mode", value: "LIVE" }}
        center={
          workspace
            ? { label: "Rack", value: workspace.name }
            : surface === "perform"
            ? { label: "Active", value: active.title }
            : { label: "Workspace", value: "Configure" }
        }
        right={
          workspace
            ? { label: "Editor", value: "Node graph" }
            : surface === "perform"
            ? { label: "Context", value: active.detail }
            : { label: "Library", value: liveContext }
        }
      />
      <div className="live-toolbar">
        <div>
          <span className="eyebrow accent">Performance workspace</span>
          <h1>LIVE</h1>
        </div>
        <div className="live-surface-tabs" role="tablist" aria-label="LIVE views">
          <button
            className={surface === "perform" ? "active" : ""}
            onClick={() => onSurfaceChange("perform")}
          >
            Perform
          </button>
          <button
            className={surface === "configure" ? "active" : ""}
            onClick={() => onSurfaceChange("configure")}
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
          onWorkspaceChange={handleWorkspaceChange}
          renderPluginSurface={renderPluginSurface}
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
        <section className="sequence-group setlist-target-group" key={setlist.id}>
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
  onWorkspaceChange,
  renderPluginSurface,
}: {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot;
  pending: boolean;
  onWorkspaceChange: (workspace: { kind: "rack"; name: string } | null) => void;
  renderPluginSurface: LivePageProps["renderPluginSurface"];
}) {
  const [kind, setKind] = useState<ConfigKind>("rack");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editorDirty, setEditorDirty] = useState(false);
  const [rackWorkspaceId, setRackWorkspaceId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PendingPerformanceDelete | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBusyRef = useRef(false);
  const items =
    kind === "rack"
      ? performance.library.racks
      : kind === "song"
        ? performance.library.songs
        : performance.library.setlists;
  const selectedExists = selectedId === "new"
    || items.some((item) => item.id === selectedId);
  const activeSelectedId = selectedExists ? selectedId : null;
  const activeRackWorkspaceId = rackWorkspaceId === "new"
    || performance.library.racks.some((rack) => rack.id === rackWorkspaceId)
    ? rackWorkspaceId
    : null;
  const pendingDeleteCollection = pendingDelete?.kind === "rack"
    ? performance.library.racks
    : pendingDelete?.kind === "song"
      ? performance.library.songs
      : performance.library.setlists;
  const activePendingDelete = pendingDelete && pendingDeleteCollection.some(
    (item) => item.id === pendingDelete.id,
  ) ? pendingDelete : null;
  const deleteRackPlan = activePendingDelete?.kind === "rack"
    ? rackCascadePlan(performance, activePendingDelete.id)
    : null;

  useEffect(() => {
    if (!editorDirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [editorDirty]);

  const workspaceRackName = activeRackWorkspaceId === "new"
    ? "New Rack"
    : performance.library.racks.find((rack) => rack.id === activeRackWorkspaceId)?.name;
  useEffect(() => {
    const workspace = workspaceRackName
      ? { kind: "rack" as const, name: workspaceRackName }
      : null;
    onWorkspaceChange(workspace);
  }, [onWorkspaceChange, workspaceRackName]);
  useEffect(
    () => () => onWorkspaceChange(null),
    [onWorkspaceChange],
  );

  const proceed = useCallback((action: () => void) => {
    if (
      editorDirty &&
      !window.confirm("Discard the unsaved changes in this editor?")
    )
      return;
    action();
  }, [editorDirty]);
  useEffect(() => {
    const closeWorkspace = () => {
      proceed(() => {
        setRackWorkspaceId(null);
        setSelectedId(null);
      });
    };
    window.addEventListener("rackforge:close-rack-workspace", closeWorkspace);
    return () => window.removeEventListener("rackforge:close-rack-workspace", closeWorkspace);
  }, [proceed]);
  const selectItem = (id: string) => {
    if (selectedId === id && kind !== "rack") return;
    proceed(() => {
      setSelectedId(id);
      if (kind === "rack") setRackWorkspaceId(id);
    });
  };
  const changeKind = (next: ConfigKind) => {
    if (next === kind) return;
    proceed(() => {
    setRackWorkspaceId(null);
    setKind(next);
    setSelectedId(null);
    });
  };
  const requestDelete = (item: { id: string; name: string }) => {
    setDeleteError(null);
    setPendingDelete({ kind, id: item.id, name: item.name });
  };
  const confirmDelete = async (cascade: boolean) => {
    if (!pendingDelete || deleteBusyRef.current || pending) return;
    const hasRackDependencies = !!deleteRackPlan && (
      deleteRackPlan.dependentRacks.length > 0 ||
      deleteRackPlan.songs.length > 0 ||
      deleteRackPlan.setlists.length > 0
    );
    if (hasRackDependencies && !cascade) {
      setDeleteError("Confirm the dependent Rack, Song and Setlist cleanup to delete this Rack.");
      return;
    }
    const edit: PerformanceEdit = pendingDelete.kind === "rack"
      ? { kind: "delete_rack", rack_id: pendingDelete.id }
      : pendingDelete.kind === "song"
        ? { kind: "delete_song", song_id: pendingDelete.id }
        : { kind: "delete_setlist", setlist_id: pendingDelete.id };
    deleteBusyRef.current = true;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      let revision = performance.revision;
      if (pendingDelete.kind === "rack" && deleteRackPlan && hasRackDependencies) {
        for (const affected of deleteRackPlan.setlists) {
          const snapshot = await dispatchEdit(
            revision,
            affected.next
              ? { kind: "put_setlist", setlist: affected.next }
              : { kind: "delete_setlist", setlist_id: affected.current.id },
          );
          revision = snapshot.revision;
        }
        for (const song of deleteRackPlan.songs) {
          const snapshot = await dispatchEdit(revision, {
            kind: "delete_song",
            song_id: song.id,
          });
          revision = snapshot.revision;
        }
        for (const rackId of deleteRackPlan.rackDeleteOrder) {
          const snapshot = await dispatchEdit(revision, {
            kind: "delete_rack",
            rack_id: rackId,
          });
          revision = snapshot.revision;
        }
      } else {
        await dispatchEdit(revision, edit);
      }
      setPendingDelete(null);
    } catch (reason) {
      setDeleteError(
        reason instanceof Error ? reason.message : `Could not delete ${pendingDelete.name}.`,
      );
    } finally {
      deleteBusyRef.current = false;
      setDeleteBusy(false);
    }
  };

  if (activeSelectedId === null) {
    return (
      <>
      <div className="live-browser config-library-browser">
        <div className="live-mode-tabs" role="tablist" aria-label="Configuration type">
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
        <div className="config-library-heading">
          <div>
            <span className="card-kicker">Performance library</span>
            <h2>{kindLabels[kind]}</h2>
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
        </div>
        {items.length === 0 ? (
          <div className="config-library-empty">
            <strong>No {kindLabels[kind]} yet</strong>
            <p>Create one with the New {kindLabels[kind].slice(0, -1)} button.</p>
          </div>
        ) : (
          <div className="target-grid config-target-grid">
            {items.map((item, index) => {
              const detail = kind === "rack"
                ? `${"slots" in item ? item.slots.length : 0} slots`
                : kind === "song"
                  ? `${"parts" in item ? item.parts.length : 0} parts`
                  : `${"entries" in item ? item.entries.length : 0} entries`;
              return (
                <article className="target-card config-target-card" key={item.id}>
                  <span className="target-index">{String(index + 1).padStart(2, "0")}</span>
                  <div>
                    <h3>{item.name}</h3>
                    <p>{detail} · {item.enabled ? "Enabled" : "Disabled"}</p>
                  </div>
                  <div className="config-card-actions">
                    <button className="config-card-edit" onClick={() => selectItem(item.id)}>
                      Edit
                    </button>
                    <button
                      className="config-card-delete"
                      disabled={pending}
                      onClick={() => requestDelete(item)}
                    >
                      Delete
                    </button>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </div>
      {activePendingDelete ? (
        <PerformanceDeleteDialog
          target={activePendingDelete}
          rackPlan={deleteRackPlan}
          deleting={deleteBusy || pending}
          error={deleteError}
          onClose={() => {
            if (deleteBusy || pending) return;
            setPendingDelete(null);
            setDeleteError(null);
          }}
          onConfirm={confirmDelete}
        />
      ) : null}
      </>
    );
  }

  return (
    <div className={`performance-config editor-workspace${activeRackWorkspaceId ? " rack-graph-workspace" : ""}`}>
      <main className="config-editor">
        {kind === "rack" && (
          <RackEditor
            key={`rack:${activeSelectedId ?? "empty"}`}
            rack={
              activeSelectedId === "new"
                ? newRack()
                : performance.library.racks.find((item) => item.id === activeSelectedId)
            }
            instances={session?.instances ?? []}
            session={session}
            performance={performance}
            pending={pending}
            immersive={activeRackWorkspaceId !== null}
            renderPluginSurface={renderPluginSurface}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => {
              setSelectedId(id);
              setRackWorkspaceId(id);
            }}
            onDeleted={() => {
              setSelectedId(null);
              setRackWorkspaceId(null);
            }}
          />
        )}
        {kind === "song" && (
          <SongEditor
            key={`song:${activeSelectedId ?? "empty"}`}
            song={
              activeSelectedId === "new"
                ? newSong(performance)
                : performance.library.songs.find((item) => item.id === activeSelectedId)
            }
            performance={performance}
            pending={pending}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => setSelectedId(id)}
            onDeleted={() => setSelectedId(null)}
          />
        )}
        {kind === "setlist" && (
          <SetlistEditor
            key={`setlist:${activeSelectedId ?? "empty"}`}
            setlist={
              activeSelectedId === "new"
                ? newSetlist(performance)
                : performance.library.setlists.find(
                    (item) => item.id === activeSelectedId,
                  )
            }
            performance={performance}
            pending={pending}
            onDirtyChange={setEditorDirty}
            onSaved={(id) => setSelectedId(id)}
            onDeleted={() => setSelectedId(null)}
          />
        )}
      </main>
    </div>
  );
}

function PerformanceDeleteDialog({
  target,
  rackPlan,
  deleting,
  error,
  onClose,
  onConfirm,
}: {
  target: PendingPerformanceDelete;
  rackPlan: RackCascadePlan | null;
  deleting: boolean;
  error: string | null;
  onClose: () => void;
  onConfirm: (cascade: boolean) => Promise<void>;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const [cascade, setCascade] = useState(false);
  const label = kindLabels[target.kind].slice(0, -1);
  const dependencyCount = rackPlan
    ? rackPlan.dependentRacks.length + rackPlan.songs.length + rackPlan.setlists.length
    : 0;
  const hasDependencies = dependencyCount > 0;
  return (
    <div
      className="preset-modal-backdrop performance-delete-backdrop"
      onMouseDown={(event) => {
        if (!deleting && event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="preset-modal performance-delete-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
      >
        <header className="preset-modal-header">
          <div>
            <span className="eyebrow">Performance library</span>
            <h2 id={titleId}>Delete {target.name}?</h2>
          </div>
          <button className="preset-modal-close" disabled={deleting} onClick={onClose} aria-label="Close">×</button>
        </header>
        <div className="performance-delete-copy" id={descriptionId}>
          <p>This permanently removes the {label} from the RackForge performance library.</p>
          {hasDependencies && rackPlan ? (
            <fieldset className="performance-delete-dependencies" disabled={deleting}>
              <legend>Dependent performance items</legend>
              <p>
                {rackPlan.dependentRacks.length} Racks · {rackPlan.songs.length} Songs ·{" "}
                {rackPlan.setlists.reduce((count, item) => count + item.removedEntries, 0)} Setlist entries
              </p>
              <label>
                <input
                  type="checkbox"
                  checked={cascade}
                  onChange={(event) => setCascade(event.target.checked)}
                />
                <span>
                  <strong>Remove these dependencies too</strong>
                  <small>
                    Dependent Racks and Songs will be deleted. Setlist entries will be removed;
                    empty Setlists will also be deleted.
                  </small>
                </span>
              </label>
            </fieldset>
          ) : (
            <p>No Song, Setlist or parent Rack depends on this {label}.</p>
          )}
          {error ? <p className="form-error">{error}</p> : null}
        </div>
        <footer className="performance-delete-actions">
          <button className="secondary-button" disabled={deleting} onClick={onClose}>Cancel</button>
          <button
            className="danger-button"
            disabled={deleting || (hasDependencies && !cascade)}
            onClick={() => void onConfirm(cascade)}
          >
            <AsyncActionLabel active={deleting} activeLabel={`Deleting ${label}…`}>
              Delete {label}
            </AsyncActionLabel>
          </button>
        </footer>
      </section>
    </div>
  );
}

function defaultSlot(instances: PluginInstance[]): RackSlot {
  const instance = instances[0];
  return {
    id: performanceId("slot"),
    name: instance?.plugin_name ?? "Instrument",
    plugin_id: instance?.plugin_id ?? "org.rackforge.missing",
    legacy_program_id: instance?.selected_sound_id,
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

function newRack(): RackDefinition {
  const slots: RackSlot[] = [];
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
        {pending ? (
          <small className="async-status-line">
            <AsyncSpinner label="Applying changes…" />
            <span>Applying changes…</span>
          </small>
        ) : (
          <small>{dirty ? "Unsaved changes" : "Saved"}</small>
        )}
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
          Save
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

function RackWorkspaceDetails({
  name,
  enabled,
  dirty,
  isNew,
  pending,
  instrumentCount,
  previewStatus,
  onName,
  onEnabled,
  onSave,
  onCancel,
  onDismiss,
  mobile = false,
}: {
  name: string;
  enabled: boolean;
  dirty: boolean;
  isNew: boolean;
  pending: boolean;
  instrumentCount: number;
  previewStatus: "idle" | "applying" | "ready";
  onName: (name: string) => void;
  onEnabled: (enabled: boolean) => void;
  onSave: () => void;
  onCancel: () => void;
  onDismiss?: () => void;
  mobile?: boolean;
}) {
  return (
    <section
      className={`rack-workspace-details${mobile ? " mobile" : ""}`}
      aria-label="Rack details"
    >
      <header>
        <div>
          <span className="card-kicker">Rack details</span>
          <strong>{dirty || isNew ? "Unsaved changes" : "Saved"}</strong>
        </div>
        {onDismiss ? (
          <button type="button" className="rack-details-dismiss" onClick={onDismiss} aria-label="Close Rack details">
            ×
          </button>
        ) : null}
      </header>
      <div className="rack-details-fields">
        <label>
          <span>Name</span>
          <input
            value={name}
            maxLength={64}
            autoComplete="off"
            onChange={(event) => onName(event.target.value)}
          />
        </label>
        <label className="rack-details-toggle">
          <span>
            <strong>Available in LIVE</strong>
            <small>{instrumentCount} {instrumentCount === 1 ? "instrument" : "instruments"}</small>
          </span>
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => onEnabled(event.target.checked)}
          />
          <i />
        </label>
      </div>
      <footer>
        <span className={`rack-details-preview ${previewStatus}`}>
          {previewStatus === "applying"
            ? "Applying preview…"
            : previewStatus === "ready"
              ? "Preview active"
              : "Preview idle"}
        </span>
        <div>
          <button type="button" className="secondary-button" disabled={pending} onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className="save-button"
            disabled={(!dirty && !isNew) || pending}
            onClick={onSave}
          >
            <AsyncActionLabel active={pending} activeLabel="Saving…">Save</AsyncActionLabel>
          </button>
        </div>
      </footer>
    </section>
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

function rackPreviewVoiceCount(rack: RackDefinition | undefined) {
  if (!rack) return 0;
  const enabledSlots = new Set(
    rack.slots.filter((slot) => slot.enabled).map((slot) => slot.id),
  );
  return materializeRackGraph(rack).graph!.nodes.filter(
    (node) =>
      node.kind.kind === "plugin" && enabledSlots.has(node.kind.slot_id),
  ).length;
}

function RackEditor({
  rack,
  instances,
  session,
  performance,
  pending,
  immersive,
  renderPluginSurface,
  onDirtyChange,
  onSaved,
  onDeleted,
}: {
  rack?: RackDefinition;
  instances: PluginInstance[];
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot;
  pending: boolean;
  immersive: boolean;
  renderPluginSurface: LivePageProps["renderPluginSurface"];
  onDirtyChange: (dirty: boolean) => void;
  onSaved: (id: string) => void;
  onDeleted: (nextId: string | null) => void;
}) {
  const original = rack ? materializeRackGraph(rack) : undefined;
  const [draft, setDraft] = useState(() =>
    original ? clone(original) : undefined,
  );
  const [baseRevision, setBaseRevision] = useState(performance.revision);
  const [error, setError] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const previewSupported = !isNativeHost();
  const initialPreviewMode = session?.active_mode ?? "idle";
  const previewOriginRef = useRef({
    mode: initialPreviewMode,
    active: performance.live.active,
  });
  const previewSequenceRef = useRef(0);
  const previewEngagedRef = useRef(false);
  const previewModeLiveRef = useRef(initialPreviewMode === "live");
  const [previewStatus, setPreviewStatus] = useState<"idle" | "applying" | "ready">("idle");
  const [previewError, setPreviewError] = useState<string | null>(null);
  const previewId = draft?.id;
  // Keep an immutable payload for the debounced preview. Depending on a
  // hand-picked fingerprint and reading the latest value from a ref allowed
  // graph edits to be visually committed without publishing the new Rack to
  // Core. The serialized draft makes every audible graph/state change part of
  // the effect dependency and guarantees that the timer sends that exact
  // revision.
  const transportDraft = draft ? normalizeRackGraphGeometry(draft) : undefined;
  const previewPayload = transportDraft ? JSON.stringify(transportDraft) : null;
  const previewVoiceCount = rackPreviewVoiceCount(transportDraft);
  const visiblePreviewStatus = previewVoiceCount === 0 ? "idle" : previewStatus;
  const visiblePreviewError = previewVoiceCount === 0 ? null : previewError;
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(original);
  const isNew = !!draft && !performance.library.racks.some((item) => item.id === draft.id);
  useEffect(() => {
    onDirtyChange(dirty || isNew);
    return () => onDirtyChange(false);
  }, [dirty, isNew, onDirtyChange]);
  useEffect(() => {
    if (!previewSupported || !previewId) return;
    const origin = previewOriginRef.current;
    return () => {
      previewSequenceRef.current += 1;
      if (!previewEngagedRef.current) return;
      if (origin.mode === "live" && origin.active) {
        dispatchCommand({ type: "set_active_mode", mode: "live" });
        dispatchCommand({ type: "activate_live_target", location: origin.active });
      } else if (origin.mode === "live") {
        dispatchCommand({ type: "set_active_mode", mode: "idle" });
        dispatchCommand({ type: "set_active_mode", mode: "live" });
      } else {
        dispatchCommand({ type: "set_active_mode", mode: origin.mode });
      }
    };
  }, [previewId, previewSupported]);
  useEffect(() => {
    if (!previewSupported || previewPayload === null) return;
    const sequence = ++previewSequenceRef.current;
    if (previewVoiceCount === 0) {
      if (!previewEngagedRef.current || !previewModeLiveRef.current) return;
      previewModeLiveRef.current = false;
      void dispatchCommandAwait({ type: "set_active_mode", mode: "idle" }).catch((reason) => {
        if (previewSequenceRef.current !== sequence) return;
        setPreviewError(
          reason instanceof Error ? reason.message : "Could not silence the empty Rack preview.",
        );
      });
      return;
    }
    const timer = window.setTimeout(() => {
      const previewRack = JSON.parse(previewPayload) as RackDefinition;
      setPreviewStatus("applying");
      setPreviewError(null);
      void (async () => {
        try {
          if (!previewModeLiveRef.current) {
            previewEngagedRef.current = true;
            previewModeLiveRef.current = true;
            await dispatchCommandAwait({ type: "set_active_mode", mode: "live" });
            if (previewSequenceRef.current !== sequence) return;
          }
          if (previewSequenceRef.current !== sequence) return;
          previewEngagedRef.current = true;
          await dispatchCommandAwait({ type: "preview_rack", rack: previewRack });
          if (previewSequenceRef.current === sequence) setPreviewStatus("ready");
        } catch (reason) {
          if (previewSequenceRef.current !== sequence) return;
          previewModeLiveRef.current = false;
          setPreviewStatus("idle");
          setPreviewError(
            reason instanceof Error ? reason.message : "Could not preview this Rack.",
          );
        }
      })();
    }, 120);
    return () => window.clearTimeout(timer);
  }, [previewPayload, previewSupported, previewVoiceCount]);
  const addInstrument = useCallback((position?: RackGraphPosition) => {
    if (!draft) return;
    setDraft(addSlotToRack(draft, defaultSlot(instances), position));
  }, [draft, instances]);
  const validate = useCallback(() => {
    if (!draft) return "Select a Rack or create a new one.";
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
  }, [draft, instances]);
  const save = useCallback(async () => {
    if (!draft) return;
    const nextError = validate();
    setError(nextError);
    if (nextError) return;
    try {
      const rackToSave = normalizeRackGraphGeometry(draft);
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "put_rack",
        rack: rackToSave,
      });
      const saved = snapshot.library.racks.find((item) => item.id === draft.id);
      if (saved) setDraft(clone(materializeRackGraph(saved)));
      setBaseRevision(snapshot.revision);
      onSaved(draft.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save Rack.");
    }
  }, [baseRevision, draft, onSaved, validate]);
  useEffect(() => {
    if (!immersive) return;
    const saveWorkspace = () => void save();
    const openDetails = () => setDetailsOpen(true);
    window.addEventListener("rackforge:save-rack-workspace", saveWorkspace);
    window.addEventListener("rackforge:open-rack-details", openDetails);
    return () => {
      window.removeEventListener("rackforge:save-rack-workspace", saveWorkspace);
      window.removeEventListener("rackforge:open-rack-details", openDetails);
    };
  }, [immersive, save]);

  if (!draft)
    return <EditorEmpty>Select a Rack or create a new one.</EditorEmpty>;

  const updateSlot = (index: number, next: RackSlot) => {
    const slots = [...draft.slots];
    slots[index] = next;
    setDraft({ ...draft, slots });
  };
  const usedBy = [
    ...performance.library.songs
      .filter((song) => song.parts.some((part) => part.rack_id === draft.id))
      .map((song) => song.name),
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
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "delete_rack",
        rack_id: draft.id,
      });
      onDeleted(snapshot.library.racks[0]?.id ?? null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not delete Rack.");
    }
  };
  const cancelWorkspace = () => {
    setDetailsOpen(false);
    window.dispatchEvent(new Event("rackforge:close-rack-workspace"));
  };
  const workspaceDetails = (mobile = false) => (
    <RackWorkspaceDetails
      name={draft.name}
      enabled={draft.enabled}
      dirty={dirty}
      isNew={isNew}
      pending={pending}
      instrumentCount={previewVoiceCount}
      previewStatus={visiblePreviewStatus}
      onName={(name) => setDraft({ ...draft, name })}
      onEnabled={(enabled) => setDraft({ ...draft, enabled })}
      onSave={() => void save()}
      onCancel={cancelWorkspace}
      onDismiss={mobile ? () => setDetailsOpen(false) : undefined}
      mobile={mobile}
    />
  );

  return (
    <form
      className={`performance-form rack-editor-form${immersive ? " immersive" : ""}`}
      onSubmit={(event) => event.preventDefault()}
    >
      <EditorHeader
        eyebrow={isNew ? "New Rack" : "Rack configuration"}
        title={draft.name}
        dirty={dirty || isNew}
        pending={pending}
        onSave={save}
        onReset={() => {
          setDraft(original ? clone(original) : newRack());
          setBaseRevision(performance.revision);
          setError(null);
        }}
        onDelete={isNew ? undefined : remove}
      />
      {error && <div className="form-error">{error}</div>}
      {visiblePreviewError ? <div className="form-error">Rack preview: {visiblePreviewError}</div> : null}
      {visiblePreviewStatus !== "idle" ? (
        <div className={`rack-preview-status ${visiblePreviewStatus}`} role="status" aria-live="polite">
          {visiblePreviewStatus === "applying" ? (
            <><AsyncSpinner label="Applying Rack preview…" /><span>Applying Rack preview…</span></>
          ) : (
            <><i /><span>{previewVoiceCount} {previewVoiceCount === 1 ? "instrument" : "instruments"} active in preview</span></>
          )}
        </div>
      ) : null}
      <BasicFields
        name={draft.name}
        enabled={draft.enabled}
        onName={(name) => setDraft({ ...draft, name })}
        onEnabled={(enabled) => setDraft({ ...draft, enabled })}
      />
      {immersive ? <aside className="rack-workspace-details-panel">{workspaceDetails()}</aside> : null}
      {immersive && detailsOpen ? (
        <div
          className="rack-details-sheet-backdrop"
          onPointerDown={(event) => {
            if (event.target === event.currentTarget) setDetailsOpen(false);
          }}
        >
          {workspaceDetails(true)}
        </div>
      ) : null}
      <EditorSection
        title="Rack graph"
        detail="Route instruments and child Racks. Positions and labels are portable; the viewport stays local to this device."
        action={null}
      >
        <Suspense fallback={<div className="rack-graph-loading">Loading graph editor…</div>}>
          <RackGraphEditor
            rack={draft}
            racks={performance.library.racks}
            onChange={(update) =>
              setDraft((current) => {
                if (!current) return current;
                return typeof update === "function" ? update(current) : update;
              })
            }
            canAddInstrument={draft.slots.length < 32 && instances.length > 0}
            onAddInstrument={addInstrument}
            instances={instances}
            renderPluginSurface={renderPluginSurface}
          />
        </Suspense>
      </EditorSection>
      <EditorSection
        title="Instrument settings"
        detail="Configure plugin state, MIDI filters and mix for each instrument node."
        action={
          <button
            type="button"
            onClick={() => addInstrument()}
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
    <article id={`rack-slot-${slot.id}`} className={`slot-editor${slot.enabled ? "" : " disabled"}`}>
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
              <button type="button" disabled={!presetId || presetBusy} onClick={loadPreset}>
                <AsyncActionLabel active={presetBusy} activeLabel="Loading…">Load</AsyncActionLabel>
              </button>
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
  onDeleted: (nextId: string | null) => void;
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
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "delete_song",
        song_id: draft.id,
      });
      onDeleted(snapshot.library.songs[0]?.id ?? null);
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
  onDeleted: (nextId: string | null) => void;
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
      const snapshot = await dispatchEdit(baseRevision, {
        kind: "delete_setlist",
        setlist_id: draft.id,
      });
      onDeleted(snapshot.library.setlists[0]?.id ?? null);
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
