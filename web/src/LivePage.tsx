import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ChevronLeft, ChevronRight, LogOut, Save } from "lucide-react";
import { SequencerStrip } from "./SequencerPanel";
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
import { ModalDialog } from "./components/ModalDialog";
import { scopedId } from "./ids";
import { isNativeHost } from "./host";
import {
  addSlotToRack,
  graphFromRackReference,
  graphFromSlots,
  materializeRackGraph,
  normalizeRackGraphGeometry,
  removeSlotFromRack,
  songPartAsRack,
  songPartGraphFromRack,
} from "./rackGraph";
import {
  buildRackPluginInstances,
  rackPluginRole,
  type RackPluginRole,
} from "./rackPluginSelection";
import { PluginPickerDialog } from "./components/PluginPickerDialog";
import type {
  LiveBrowseMode,
  LiveLocation,
  HostPresetSummary,
  PerformanceEdit,
  PerformanceSnapshot,
  PluginInstance,
  PluginWebDescriptor,
  PluginStateReference,
  RackDefinition,
  RackGraphPosition,
  RackSlot,
  SessionSnapshot,
  SetlistDefinition,
  SongDefinition,
  SongPart,
} from "./types";

const RackGraphEditor = lazy(() => import("./components/RackGraphEditor"));

type ConfigKind = "rack" | "song" | "setlist";

export interface PerformanceGraphWorkspace {
  kind: "rack" | "song_part";
  name: string;
}

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
  plugins: PluginWebDescriptor[];
  pending: boolean;
  surface: "perform" | "configure";
  onSurfaceChange: (surface: "perform" | "configure") => void;
  onWorkspaceChange: (workspace: PerformanceGraphWorkspace | null) => void;
  renderPluginSurface: (options: {
    instance: PluginInstance;
    state?: PluginStateReference;
    onStateChange: (state: PluginStateReference) => void;
    onSelectSound: (soundId: string) => Promise<unknown>;
    parameterLinkInstanceId: string;
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
    song.parts.some((part) =>
      part.content
        ? part.content.graph.nodes.some(
            (node) => node.kind.kind === "rack" && rackIds.has(node.kind.rack_id),
          )
        : rackIds.has(part.rack_id),
    ),
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
  if (!left || left.kind !== right.kind) return false;
  if (left.kind === "rack" && right.kind === "rack") {
    return left.rack_id === right.rack_id;
  }
  if (left.kind === "song" && right.kind === "song") {
    return left.song_id === right.song_id && left.part_id === right.part_id;
  }
  if (left.kind === "setlist" && right.kind === "setlist") {
    return left.setlist_id === right.setlist_id
      && left.entry_id === right.entry_id
      && left.part_id === right.part_id;
  }
  return false;
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
  plugins,
  pending,
  surface,
  onSurfaceChange,
  onWorkspaceChange,
  renderPluginSurface,
}: LivePageProps) {
  const [workspace, setWorkspace] = useState<PerformanceGraphWorkspace | null>(null);
  const handleWorkspaceChange = useCallback(
    (next: PerformanceGraphWorkspace | null) => {
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
            ? {
                label: workspace.kind === "rack" ? "Rack" : "Song Part",
                value: workspace.name,
              }
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
      {performance ? <SequencerStrip performance={performance} /> : null}
      {!performance ? (
        <LiveLoading />
      ) : surface === "perform" ? (
        <PerformanceBrowser session={session} performance={performance} />
      ) : (
        <PerformanceConfig
          session={session}
          performance={performance}
          plugins={plugins}
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
  const initializedBrowseMode = useRef(false);
  useEffect(() => {
    if (initializedBrowseMode.current) return;
    initializedBrowseMode.current = true;
    if (!performance.live.active && performance.live.mode !== "rack") {
      dispatchCommand({ type: "set_live_browse_mode", mode: "rack" });
    }
  }, [performance.live.active, performance.live.mode]);
  const mode = performance.live.mode;
  const active = describeLocation(performance, performance.live.active);
  const activate = (location: LiveLocation) => {
    if (session?.active_mode !== "live") {
      dispatchCommand({ type: "set_active_mode", mode: "live" });
    }
    dispatchCommand({ type: "activate_live_target", location });
  };
  const changeMode = (nextMode: LiveBrowseMode) => {
    dispatchCommand({ type: "set_live_browse_mode", mode: nextMode });
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
              className={`entity-${item}${mode === item ? " active" : ""}`}
              onClick={() => changeMode(item)}
              role="tab"
              aria-selected={mode === item}
            >
              {item.toUpperCase()}
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
  label = "Load",
  onClick,
}: {
  active: boolean;
  disabled?: boolean;
  label?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`activate-button live-item-state${active ? " active" : ""}`}
      disabled={disabled || active}
      onClick={onClick}
    >
      {active ? "PLAYING" : label.toUpperCase()}
    </button>
  );
}

function playableSongParts(performance: PerformanceSnapshot, song: SongDefinition) {
  return song.parts.filter((part) => {
    if (part.content) return true;
    return performance.library.racks.some(
      (rack) => rack.id === part.rack_id && rack.enabled,
    );
  });
}

function StageNavigator({
  kind,
  title,
  detail,
  position,
  previousLabel,
  nextLabel,
  onPrevious,
  onNext,
}: {
  kind: "rack" | "song" | "setlist";
  title: string;
  detail: string;
  position: string;
  previousLabel: string;
  nextLabel: string;
  onPrevious?: () => void;
  onNext?: () => void;
}) {
  return (
    <header className={`live-stage-navigator entity-${kind}`}>
      <button
        type="button"
        className="live-stage-step"
        disabled={!onPrevious}
        onClick={onPrevious}
        aria-label={previousLabel}
      >
        <ChevronLeft aria-hidden="true" />
      </button>
      <div className="live-stage-heading">
        <span className="card-kicker">{kind}</span>
        <strong>{title}</strong>
        <small>{detail}</small>
      </div>
      <span className="live-stage-position">{position}</span>
      <button
        type="button"
        className="live-stage-step"
        disabled={!onNext}
        onClick={onNext}
        aria-label={nextLabel}
      >
        <ChevronRight aria-hidden="true" />
      </button>
    </header>
  );
}

function SongPartTargets({
  performance,
  song,
  locationForPart,
  activate,
}: {
  performance: PerformanceSnapshot;
  song: SongDefinition;
  locationForPart: (part: SongPart) => LiveLocation;
  activate: (location: LiveLocation) => void;
}) {
  const parts = playableSongParts(performance, song);
  if (parts.length === 0) return <LiveEmpty label="This Song has no playable Parts" />;
  return (
    <div className="live-part-list" aria-label={`${song.name} parts`}>
      {parts.map((part, index) => {
        const location = locationForPart(part);
        const playing = sameLocation(performance.live.active, location);
        return (
          <button
            type="button"
            className={`live-part-target live-selectable-item entity-song-part${playing ? " active" : ""}`}
            key={part.id}
            onClick={() => activate(location)}
            aria-pressed={playing}
          >
            <span className="live-part-index">{String(index + 1).padStart(2, "0")}</span>
            <span className="live-part-copy">
              <strong>{part.name}</strong>
              <small>{part.content ? "Part graph" : "Rack"}</small>
            </span>
            <span className="live-part-state live-item-state">
              {playing ? "PLAYING" : "LOAD"}
            </span>
          </button>
        );
      })}
    </div>
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
  const selectedRackId = performance.live.rack?.kind === "rack"
    ? performance.live.rack.rack_id
    : undefined;
  const selectedIndex = racks.findIndex((rack) => rack.id === selectedRackId);
  const selectedRack = selectedIndex >= 0 ? racks[selectedIndex] : undefined;
  return (
    <div className="live-target-workspace">
      {selectedRack ? (
        <StageNavigator
          kind="rack"
          title={selectedRack.name}
          detail={`${selectedRack.slots.filter((slot) => slot.enabled).length} active slots`}
          position={`${selectedIndex + 1} / ${racks.length}`}
          previousLabel="Load previous Rack"
          nextLabel="Load next Rack"
          onPrevious={selectedIndex > 0
            ? () => activate({ kind: "rack", rack_id: racks[selectedIndex - 1].id })
            : undefined}
          onNext={selectedIndex < racks.length - 1
            ? () => activate({ kind: "rack", rack_id: racks[selectedIndex + 1].id })
            : undefined}
        />
      ) : null}
      <div className="target-grid">
        {racks.map((rack, index) => {
          const location: LiveLocation = { kind: "rack", rack_id: rack.id };
          const playing = sameLocation(performance.live.active, location);
          return (
            <article
              className={`target-card live-selectable-item entity-rack${playing ? " active" : ""}`}
              key={rack.id}
            >
              <span className="target-index">{String(index + 1).padStart(2, "0")}</span>
              <div>
                <h3>{rack.name}</h3>
                <p>{rack.slots.filter((slot) => slot.enabled).length} active slots</p>
              </div>
              <ActivateButton
                active={playing}
                onClick={() => activate(location)}
              />
            </article>
          );
        })}
      </div>
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
  const enabledSongs = performance.library.songs.filter((song) => song.enabled);
  if (enabledSongs.length === 0) return <LiveEmpty label="No enabled Songs" />;
  const songs = enabledSongs.filter(
    (song) => playableSongParts(performance, song).length > 0,
  );
  const selectedLocation = performance.live.song?.kind === "song"
    ? performance.live.song
    : undefined;
  const selectedIndex = songs.findIndex((song) => song.id === selectedLocation?.song_id);
  const selectedSong = selectedIndex >= 0 ? songs[selectedIndex] : undefined;
  const activateSong = (song: SongDefinition) => {
    const part = playableSongParts(performance, song)[0];
    if (part) activate({ kind: "song", song_id: song.id, part_id: part.id });
  };

  if (selectedSong) {
    return (
      <div className="live-target-workspace">
        <StageNavigator
          kind="song"
          title={selectedSong.name}
          detail={`${playableSongParts(performance, selectedSong).length} Parts`}
          position={`${selectedIndex + 1} / ${songs.length}`}
          previousLabel="Load previous Song"
          nextLabel="Load next Song"
          onPrevious={selectedIndex > 0 ? () => activateSong(songs[selectedIndex - 1]) : undefined}
          onNext={selectedIndex < songs.length - 1
            ? () => activateSong(songs[selectedIndex + 1])
            : undefined}
        />
        <SongPartTargets
          performance={performance}
          song={selectedSong}
          locationForPart={(part) => ({
            kind: "song",
            song_id: selectedSong.id,
            part_id: part.id,
          })}
          activate={activate}
        />
      </div>
    );
  }

  return (
    <div className="target-grid live-song-picker">
      {enabledSongs.map((song, index) => {
        const parts = playableSongParts(performance, song);
        return (
          <article className="target-card live-selectable-item entity-song" key={song.id}>
            <span className="target-index">{String(index + 1).padStart(2, "0")}</span>
            <div>
              <h3>{song.name}</h3>
              <p>{parts.length} playable Parts</p>
            </div>
            <ActivateButton
              active={false}
              disabled={parts.length === 0}
              label="Open"
              onClick={() => activateSong(song)}
            />
          </article>
        );
      })}
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
  const [dismissedSetlistId, setDismissedSetlistId] = useState<string | null>(null);
  const selectedLocation = performance.live.setlist?.kind === "setlist"
    ? performance.live.setlist
    : undefined;
  const selectedSetlistId = selectedLocation?.setlist_id;
  useEffect(() => {
    const exitSetlist = () => {
      if (selectedSetlistId) setDismissedSetlistId(selectedSetlistId);
    };
    window.addEventListener("rackforge:exit-live-setlist", exitSetlist);
    return () => window.removeEventListener("rackforge:exit-live-setlist", exitSetlist);
  }, [selectedSetlistId]);
  if (setlists.length === 0) return <LiveEmpty label="No enabled Setlists" />;

  const selectedSetlist = selectedLocation?.setlist_id !== dismissedSetlistId
    ? setlists.find((setlist) => setlist.id === selectedLocation?.setlist_id)
    : undefined;
  const playableEntries = selectedSetlist?.entries.flatMap((entry) => {
    const song = performance.library.songs.find((item) => item.id === entry.song_id);
    return song?.enabled && playableSongParts(performance, song).length > 0
      ? [{ entry, song }]
      : [];
  }) ?? [];
  const selectedEntryIndex = playableEntries.findIndex(
    ({ entry }) => entry.id === selectedLocation?.entry_id,
  );
  const selectedEntry = selectedEntryIndex >= 0
    ? playableEntries[selectedEntryIndex]
    : playableEntries[0];
  const activateEntry = (index: number) => {
    const target = playableEntries[index];
    const part = target && playableSongParts(performance, target.song)[0];
    if (selectedSetlist && target && part) {
      activate({
        kind: "setlist",
        setlist_id: selectedSetlist.id,
        entry_id: target.entry.id,
        part_id: part.id,
      });
    }
  };

  if (selectedSetlist && selectedEntry) {
    const currentIndex = Math.max(0, selectedEntryIndex);
    return (
      <div className="live-target-workspace">
        <StageNavigator
          kind="setlist"
          title={selectedSetlist.name}
          detail={selectedEntry.song.name}
          position={`${currentIndex + 1} / ${playableEntries.length}`}
          previousLabel="Load previous Setlist Song"
          nextLabel="Load next Setlist Song"
          onPrevious={currentIndex > 0 ? () => activateEntry(currentIndex - 1) : undefined}
          onNext={currentIndex < playableEntries.length - 1
            ? () => activateEntry(currentIndex + 1)
            : undefined}
        />
        <SongPartTargets
          performance={performance}
          song={selectedEntry.song}
          locationForPart={(part) => ({
            kind: "setlist",
            setlist_id: selectedSetlist.id,
            entry_id: selectedEntry.entry.id,
            part_id: part.id,
          })}
          activate={activate}
        />
      </div>
    );
  }

  return (
    <div className="target-grid live-setlist-picker">
      {setlists.map((setlist, index) => {
        const firstPlayable = setlist.entries.flatMap((entry) => {
          const song = performance.library.songs.find((item) => item.id === entry.song_id);
          const part = song?.enabled ? playableSongParts(performance, song)[0] : undefined;
          return song && part ? [{ entry, song, part }] : [];
        })[0];
        return (
          <article className="target-card live-selectable-item entity-setlist" key={setlist.id}>
            <span className="target-index">{String(index + 1).padStart(2, "0")}</span>
            <div>
              <h3>{setlist.name}</h3>
              <p>{setlist.entries.length} Songs</p>
            </div>
            <ActivateButton
              active={false}
              disabled={!firstPlayable}
              label="Open"
              onClick={() => {
                if (!firstPlayable) return;
                setDismissedSetlistId(null);
                activate({
                  kind: "setlist",
                  setlist_id: setlist.id,
                  entry_id: firstPlayable.entry.id,
                  part_id: firstPlayable.part.id,
                });
              }}
            />
          </article>
        );
      })}
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
  plugins,
  pending,
  onWorkspaceChange,
  renderPluginSurface,
}: {
  session: SessionSnapshot | null;
  performance: PerformanceSnapshot;
  plugins: PluginWebDescriptor[];
  pending: boolean;
  onWorkspaceChange: (workspace: PerformanceGraphWorkspace | null) => void;
  renderPluginSurface: LivePageProps["renderPluginSurface"];
}) {
  const [kind, setKind] = useState<ConfigKind>("rack");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editorDirty, setEditorDirty] = useState(false);
  const [editorResetEpoch, setEditorResetEpoch] = useState(0);
  const [rackWorkspaceId, setRackWorkspaceId] = useState<string | null>(null);
  const [songPartWorkspace, setSongPartWorkspace] = useState<{ id: string; name: string } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PendingPerformanceDelete | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBusyRef = useRef(false);
  const rackInstances = buildRackPluginInstances(session?.instances ?? [], plugins);
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
  const activeSongPartWorkspace = kind === "song" && activeSelectedId !== null
    ? songPartWorkspace
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
    const workspace: PerformanceGraphWorkspace | null = workspaceRackName
      ? { kind: "rack", name: workspaceRackName }
      : activeSongPartWorkspace
        ? { kind: "song_part", name: activeSongPartWorkspace.name }
        : null;
    onWorkspaceChange(workspace);
  }, [activeSongPartWorkspace, onWorkspaceChange, workspaceRackName]);
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
      if (activeSongPartWorkspace) {
        proceed(() => {
          setSongPartWorkspace(null);
          setEditorResetEpoch((current) => current + 1);
        });
        return;
      }
      proceed(() => {
        setRackWorkspaceId(null);
        setSelectedId(null);
      });
    };
    window.addEventListener("rackforge:close-graph-workspace", closeWorkspace);
    return () => window.removeEventListener("rackforge:close-graph-workspace", closeWorkspace);
  }, [activeSongPartWorkspace, proceed]);
  const selectItem = (id: string) => {
    if (selectedId === id && kind === "setlist") return;
    proceed(() => {
      setSelectedId(id);
      if (kind === "rack") setRackWorkspaceId(id);
      setSongPartWorkspace(null);
    });
  };
  const changeKind = (next: ConfigKind) => {
    if (next === kind) return;
    proceed(() => {
      setRackWorkspaceId(null);
      setSongPartWorkspace(null);
      setKind(next);
      setSelectedId(null);
    });
  };
  const openSongPartWorkspace = useCallback((part: SongPart) => {
    setSongPartWorkspace({ id: part.id, name: part.name });
  }, []);
  const syncSongPartWorkspace = useCallback((part: Pick<SongPart, "id" | "name"> | null) => {
    setSongPartWorkspace((current) => {
      if (!part) return null;
      if (current?.id === part.id && current.name === part.name) return current;
      return { id: part.id, name: part.name };
    });
  }, []);
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
              className={`entity-${item}${kind === item ? " active" : ""}`}
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
              (kind === "song" && performance.library.racks.length === 0 && (session?.instances.length ?? 0) === 0) ||
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
                <article className={`target-card config-target-card entity-${kind}`} key={item.id}>
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
    <div className={`performance-config editor-workspace${
      activeRackWorkspaceId || activeSongPartWorkspace ? " graph-workspace" : ""
    }${activeRackWorkspaceId ? " rack-graph-workspace" : ""}${
      activeSongPartWorkspace ? " song-graph-workspace" : ""
    }`}>
      <main className="config-editor">
        {kind === "rack" && (
          <RackEditor
            key={`rack:${activeSelectedId ?? "empty"}`}
            rack={
              activeSelectedId === "new"
                ? newRack()
                : performance.library.racks.find((item) => item.id === activeSelectedId)
            }
            instances={rackInstances}
            plugins={plugins}
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
            key={`song:${activeSelectedId ?? "empty"}:${editorResetEpoch}`}
            song={
              activeSelectedId === "new"
                ? newSong(performance)
                : performance.library.songs.find((item) => item.id === activeSelectedId)
            }
            performance={performance}
            instances={rackInstances}
            plugins={plugins}
            session={session}
            pending={pending}
            immersivePartId={activeSongPartWorkspace?.id ?? null}
            renderPluginSurface={renderPluginSurface}
            onDirtyChange={setEditorDirty}
            onEditPart={openSongPartWorkspace}
            onWorkspacePartChange={syncSongPartWorkspace}
            onSaved={setSelectedId}
            onDeleted={() => {
              setSelectedId(null);
              setSongPartWorkspace(null);
            }}
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
  const [cascade, setCascade] = useState(false);
  const label = kindLabels[target.kind].slice(0, -1);
  const dependencyCount = rackPlan
    ? rackPlan.dependentRacks.length + rackPlan.songs.length + rackPlan.setlists.length
    : 0;
  const hasDependencies = dependencyCount > 0;
  return (
    <ModalDialog
      eyebrow="Performance library"
      title={`Delete ${target.name}?`}
      role="alertdialog"
      onClose={onClose}
      dismissible={!deleting}
      closeLabel="Close performance deletion"
      backdropClassName="performance-delete-backdrop"
      className="performance-delete-dialog"
      actions={
        <>
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
        </>
      }
    >
        <div className="performance-delete-copy">
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
    </ModalDialog>
  );
}

function defaultSlot(instance: PluginInstance): RackSlot {
  return {
    id: performanceId("slot"),
    name: instance.plugin_name,
    plugin_id: instance.plugin_id,
    legacy_program_id: instance.selected_sound_id,
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

function newSongPart(performance: PerformanceSnapshot, name: string): SongPart {
  const rack = performance.library.racks[0];
  return {
    id: performanceId("part"),
    name,
    rack_id: rack?.id ?? "rack.song-part-placeholder",
    content: {
      slots: [],
      graph: rack ? graphFromRackReference(rack.id) : graphFromSlots([]),
    },
  };
}

function newSong(performance: PerformanceSnapshot): SongDefinition {
  return {
    schema_version: 1,
    id: performanceId("song"),
    name: "New Song",
    enabled: true,
    parts: [newSongPart(performance, "Intro")],
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
  onExit,
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
  onExit: () => void;
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
          <button type="button" className="workspace-exit-button" disabled={pending} onClick={onExit}>
            <LogOut aria-hidden="true" />
            <span>Exit</span>
          </button>
          <button
            type="button"
            className="save-button"
            disabled={(!dirty && !isNew) || pending}
            onClick={onSave}
          >
            <AsyncActionLabel active={pending} activeLabel="Saving…">
              <Save aria-hidden="true" />
              <span>Save</span>
            </AsyncActionLabel>
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

function rackPreviewVoiceCount(
  rack: RackDefinition | undefined,
  racks: RackDefinition[] = [],
  visited = new Set<string>(),
): number {
  if (!rack) return 0;
  if (visited.has(rack.id)) return 0;
  const nextVisited = new Set(visited).add(rack.id);
  const enabledSlots = new Set(
    rack.slots.filter((slot) => slot.enabled).map((slot) => slot.id),
  );
  return materializeRackGraph(rack).graph!.nodes.reduce((count, node) => {
    if (node.kind.kind === "plugin") {
      return count + Number(enabledSlots.has(node.kind.slot_id));
    }
    if (node.kind.kind !== "rack") return count;
    const childRackId = node.kind.rack_id;
    return count + rackPreviewVoiceCount(
      racks.find((candidate) => candidate.id === childRackId && candidate.enabled),
      racks,
      nextVisited,
    );
  }, 0);
}

function useSongPartPreview(
  rack: RackDefinition | undefined,
  session: SessionSnapshot | null,
  performance: PerformanceSnapshot,
) {
  const previewSupported = !isNativeHost();
  const initialMode = session?.active_mode ?? "idle";
  const originRef = useRef({ mode: initialMode, active: performance.live.active });
  const sequenceRef = useRef(0);
  const engagedRef = useRef(false);
  const modeLiveRef = useRef(initialMode === "live");
  const [status, setStatus] = useState<"idle" | "applying" | "ready">("idle");
  const [error, setError] = useState<string | null>(null);
  const transportRack = rack ? normalizeRackGraphGeometry(rack) : undefined;
  const payload = transportRack ? JSON.stringify(transportRack) : null;
  const voiceCount = rackPreviewVoiceCount(transportRack, performance.library.racks);

  const restoreOrigin = useCallback(() => {
    if (!previewSupported || !engagedRef.current) return;
    engagedRef.current = false;
    const origin = originRef.current;
    modeLiveRef.current = origin.mode === "live";
    if (origin.mode === "live" && origin.active) {
      dispatchCommand({ type: "set_active_mode", mode: "live" });
      dispatchCommand({ type: "activate_live_target", location: origin.active });
    } else if (origin.mode === "live") {
      dispatchCommand({ type: "set_active_mode", mode: "idle" });
      dispatchCommand({ type: "set_active_mode", mode: "live" });
    } else {
      dispatchCommand({ type: "set_active_mode", mode: origin.mode });
    }
  }, [previewSupported]);

  useEffect(() => () => {
    sequenceRef.current += 1;
    restoreOrigin();
  }, [restoreOrigin]);

  useEffect(() => {
    if (payload !== null) return;
    sequenceRef.current += 1;
    restoreOrigin();
  }, [payload, restoreOrigin]);

  useEffect(() => {
    if (!previewSupported || payload === null) return;
    const sequence = ++sequenceRef.current;
    if (voiceCount === 0) {
      if (!engagedRef.current || !modeLiveRef.current) return;
      modeLiveRef.current = false;
      void dispatchCommandAwait({ type: "set_active_mode", mode: "idle" }).catch((reason) => {
        if (sequenceRef.current !== sequence) return;
        setError(reason instanceof Error ? reason.message : "Could not silence the empty Part preview.");
      });
      return;
    }
    const timer = window.setTimeout(() => {
      const previewRack = JSON.parse(payload) as RackDefinition;
      setStatus("applying");
      setError(null);
      void (async () => {
        try {
          if (!modeLiveRef.current) {
            engagedRef.current = true;
            modeLiveRef.current = true;
            await dispatchCommandAwait({ type: "set_active_mode", mode: "live" });
            if (sequenceRef.current !== sequence) return;
          }
          engagedRef.current = true;
          await dispatchCommandAwait({ type: "preview_rack", rack: previewRack });
          if (sequenceRef.current === sequence) setStatus("ready");
        } catch (reason) {
          if (sequenceRef.current !== sequence) return;
          modeLiveRef.current = false;
          setStatus("idle");
          setError(reason instanceof Error ? reason.message : "Could not preview this Song Part.");
        }
      })();
    }, 120);
    return () => window.clearTimeout(timer);
  }, [payload, previewSupported, voiceCount]);

  return {
    error: voiceCount === 0 ? null : error,
    status: voiceCount === 0 ? "idle" as const : status,
    voiceCount,
  };
}

function RackEditor({
  rack,
  instances,
  plugins,
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
  plugins: PluginWebDescriptor[];
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
  const [pluginPicker, setPluginPicker] = useState<{
    position?: RackGraphPosition;
    role: RackPluginRole;
  } | null>(null);
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
  const previewVoiceCount = rackPreviewVoiceCount(
    transportDraft,
    performance.library.racks,
  );
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
  const addPlugin = useCallback((position?: RackGraphPosition, role: RackPluginRole = "instrument") => {
    setPluginPicker({ position, role });
  }, []);
  const selectPlugin = useCallback((instance: PluginInstance) => {
    // The menu says what the node is for, but the catalog says what the plugin
    // is; wire it by what it is, so a mis-aimed menu still produces a Rack that
    // works.
    const role = rackPluginRole(instance.plugin_id, plugins);
    setDraft((current) => current
      ? addSlotToRack(current, defaultSlot(instance), pluginPicker?.position, role)
      : current);
    setPluginPicker(null);
  }, [pluginPicker, plugins]);
  const handleGraphOverlayChange = useCallback((open: boolean) => {
    if (open) setDetailsOpen(false);
    window.dispatchEvent(new CustomEvent("rackforge:rack-graph-overlay", {
      detail: { open },
    }));
  }, []);
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
    window.addEventListener("rackforge:save-graph-workspace", saveWorkspace);
    window.addEventListener("rackforge:open-graph-details", openDetails);
    return () => {
      window.removeEventListener("rackforge:save-graph-workspace", saveWorkspace);
      window.removeEventListener("rackforge:open-graph-details", openDetails);
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
      .filter((song) => song.parts.some((part) =>
        part.content
          ? part.content.graph.nodes.some(
              (node) => node.kind.kind === "rack" && node.kind.rack_id === draft.id,
            )
          : part.rack_id === draft.id,
      ))
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
  const exitWorkspace = () => {
    setDetailsOpen(false);
    window.dispatchEvent(new Event("rackforge:close-graph-workspace"));
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
      onExit={exitWorkspace}
      onDismiss={mobile ? () => setDetailsOpen(false) : undefined}
      mobile={mobile}
    />
  );

  return (
    <form
      className={`performance-form rack-editor-form entity-rack${immersive ? " immersive" : ""}`}
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
            canAddInstrument={draft.slots.length < 32}
            onAddInstrument={addPlugin}
            instances={instances}
            renderPluginSurface={renderPluginSurface}
            onOverlayChange={handleGraphOverlayChange}
          />
        </Suspense>
      </EditorSection>
      <EditorSection
        title="Slot settings"
        detail="Configure plugin state and mix for each node. MIDI routing lives on an instrument's input connection; an effect is fed by the audio input instead."
        action={
          <span className="editor-section-actions">
            <button
              type="button"
              onClick={() => addPlugin(undefined, "instrument")}
              disabled={draft.slots.length >= 32}
            >
              ＋ Instrument
            </button>
            <button
              type="button"
              onClick={() => addPlugin(undefined, "effect")}
              disabled={draft.slots.length >= 32}
            >
              ＋ Effect
            </button>
          </span>
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
      {pluginPicker ? (
        <PluginPickerDialog
          instances={instances}
          plugins={plugins}
          role={pluginPicker.role}
          onSelect={selectPlugin}
          onClose={() => setPluginPicker(null)}
        />
      ) : null}
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
  allowEmpty = false,
}: {
  slot: RackSlot;
  index: number;
  total: number;
  instances: PluginInstance[];
  onChange: (slot: RackSlot) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
  allowEmpty?: boolean;
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
    <article id={`rack-slot-${slot.id}`} className={`slot-editor entity-instrument${slot.enabled ? "" : " disabled"}`}>
      <header>
        <span className="slot-number">{String(index + 1).padStart(2, "0")}</span>
        <strong>{slot.name || "Unnamed Slot"}</strong>
        <div className="reorder-controls">
          <button aria-label="Move Slot up" disabled={index === 0} onClick={() => onMove(-1)}>↑</button>
          <button aria-label="Move Slot down" disabled={index === total - 1} onClick={() => onMove(1)}>↓</button>
          <button aria-label="Remove Slot" disabled={!allowEmpty && total === 1} onClick={onRemove}>×</button>
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

function SongWorkspaceDetails({
  partName,
  dirty,
  isNew,
  pending,
  previewStatus,
  instrumentCount,
  onPartName,
  onSave,
  onExit,
  onDismiss,
  mobile = false,
}: {
  partName: string;
  dirty: boolean;
  isNew: boolean;
  pending: boolean;
  previewStatus: "idle" | "applying" | "ready";
  instrumentCount: number;
  onPartName: (name: string) => void;
  onSave: () => void;
  onExit: () => void;
  onDismiss?: () => void;
  mobile?: boolean;
}) {
  return (
    <section
      className={`rack-workspace-details song-workspace-details entity-song-part${mobile ? " mobile" : ""}`}
      aria-label="Song Part details"
    >
      <header>
        <div>
          <span className="card-kicker">Song Part details</span>
          <strong>{dirty || isNew ? "Unsaved changes" : "Saved"}</strong>
        </div>
        {onDismiss ? (
          <button type="button" className="rack-details-dismiss" onClick={onDismiss} aria-label="Close Song Part details">
            ×
          </button>
        ) : null}
      </header>
      <div className="rack-details-fields song-details-fields">
        <label>
          <span>Part name</span>
          <input
            value={partName}
            maxLength={64}
            autoComplete="off"
            onChange={(event) => onPartName(event.target.value)}
          />
        </label>
        <div className="song-part-details-summary">
          <span>Graph preview</span>
          <strong>{instrumentCount} {instrumentCount === 1 ? "active instrument" : "active instruments"}</strong>
        </div>
      </div>
      <footer>
        <span className={`rack-details-preview ${previewStatus}`}>
          {previewStatus === "applying"
            ? "Applying preview…"
            : previewStatus === "ready"
              ? `${instrumentCount} active`
              : "Preview idle"}
        </span>
        <div>
          <button type="button" className="workspace-exit-button" disabled={pending} onClick={onExit}>
            <LogOut aria-hidden="true" />
            <span>Exit</span>
          </button>
          <button
            type="button"
            className="save-button"
            disabled={(!dirty && !isNew) || pending}
            onClick={onSave}
          >
            <AsyncActionLabel active={pending} activeLabel="Saving…">
              <Save aria-hidden="true" />
              <span>Save</span>
            </AsyncActionLabel>
          </button>
        </div>
      </footer>
    </section>
  );
}

function SongEditor({
  song,
  performance,
  instances,
  plugins,
  session,
  pending,
  immersivePartId,
  renderPluginSurface,
  onDirtyChange,
  onEditPart,
  onWorkspacePartChange,
  onSaved,
  onDeleted,
}: {
  song?: SongDefinition;
  performance: PerformanceSnapshot;
  instances: PluginInstance[];
  plugins: PluginWebDescriptor[];
  session: SessionSnapshot | null;
  pending: boolean;
  immersivePartId: string | null;
  renderPluginSurface: LivePageProps["renderPluginSurface"];
  onDirtyChange: (dirty: boolean) => void;
  onEditPart: (part: SongPart) => void;
  onWorkspacePartChange: (part: Pick<SongPart, "id" | "name"> | null) => void;
  onSaved: (id: string) => void;
  onDeleted: (nextId: string | null) => void;
}) {
  const original = song;
  const [draft, setDraft] = useState(() => (song ? clone(song) : undefined));
  const [baseRevision, setBaseRevision] = useState(performance.revision);
  const [error, setError] = useState<string | null>(null);
  const [selectedPartId, setSelectedPartId] = useState(song?.parts[0]?.id);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [pluginPicker, setPluginPicker] = useState<{
    position?: RackGraphPosition;
    role: RackPluginRole;
  } | null>(null);
  const immersive = immersivePartId !== null;
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(original);
  const isNew = !!draft && !performance.library.songs.some((item) => item.id === draft.id);
  const selectedPart = draft?.parts.find(
    (part) => part.id === (immersivePartId ?? selectedPartId),
  )
    ?? draft?.parts[0];
  const selectedPartIndex = selectedPart && draft
    ? draft.parts.findIndex((part) => part.id === selectedPart.id)
    : -1;
  const selectedPartRack = selectedPart ? songPartAsRack(selectedPart) : undefined;
  const partPreview = useSongPartPreview(
    immersive ? selectedPartRack : undefined,
    session,
    performance,
  );
  useEffect(() => {
    onDirtyChange(dirty || isNew);
    return () => onDirtyChange(false);
  }, [dirty, isNew, onDirtyChange]);
  const selectedPartIdentity = selectedPart?.id;
  const selectedPartName = selectedPart?.name;
  useEffect(() => {
    if (!immersive || !selectedPartIdentity || selectedPartName === undefined) return;
    onWorkspacePartChange({ id: selectedPartIdentity, name: selectedPartName });
  }, [immersive, onWorkspacePartChange, selectedPartIdentity, selectedPartName]);
  const updatePart = useCallback((index: number, update: (part: SongPart) => SongPart) => {
    setDraft((current) => {
      if (!current || !current.parts[index]) return current;
      const parts = [...current.parts];
      parts[index] = update(parts[index]);
      return { ...current, parts };
    });
  }, []);
  const updatePartRack = useCallback((
    update: RackDefinition | ((current: RackDefinition) => RackDefinition),
  ) => {
    if (!selectedPart || selectedPartIndex < 0) return;
    updatePart(selectedPartIndex, (part) => {
      const currentRack = songPartAsRack(part);
      const nextRack = typeof update === "function" ? update(currentRack) : update;
      return {
        ...part,
        content: songPartGraphFromRack(nextRack),
      };
    });
  }, [selectedPart, selectedPartIndex, updatePart]);
  const save = useCallback(async () => {
    if (!draft) return;
    const nextError = validationName(draft.name) ??
      (draft.parts.length === 0 ? "A Song needs at least one Part." : null) ??
      (draft.parts.find((part) => validationName(part.name)) ? "Every Part needs a valid name." : null) ??
      (draft.parts.find((part) => {
        const graph = songPartAsRack(part);
        return !graph.graph?.nodes.some(
          (node) => node.kind.kind === "plugin" || node.kind.kind === "rack",
        );
      }) ? "Every Part needs at least one instrument or Rack node." : null);
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
  }, [baseRevision, draft, onSaved]);
  const handleGraphOverlayChange = useCallback((open: boolean) => {
    if (open) setDetailsOpen(false);
    window.dispatchEvent(new CustomEvent("rackforge:rack-graph-overlay", {
      detail: { open },
    }));
  }, []);
  useEffect(() => {
    if (!immersive) return;
    const saveWorkspace = () => void save();
    const openDetails = () => setDetailsOpen(true);
    window.addEventListener("rackforge:save-graph-workspace", saveWorkspace);
    window.addEventListener("rackforge:open-graph-details", openDetails);
    return () => {
      window.removeEventListener("rackforge:save-graph-workspace", saveWorkspace);
      window.removeEventListener("rackforge:open-graph-details", openDetails);
    };
  }, [immersive, save]);
  if (!draft) return <EditorEmpty>Select a Song or create a new one.</EditorEmpty>;
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
  const addPart = () => {
    const part = newSongPart(performance, `Part ${draft.parts.length + 1}`);
    setDraft({ ...draft, parts: [...draft.parts, part] });
    setSelectedPartId(part.id);
  };
  const movePart = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= draft.parts.length) return;
    const parts = [...draft.parts];
    [parts[index], parts[target]] = [parts[target], parts[index]];
    setDraft({ ...draft, parts });
  };
  const removePart = (index: number) => {
    if (draft.parts.length === 1) return;
    const removed = draft.parts[index];
    const nextParts = draft.parts.filter((item) => item.id !== removed.id);
    setDraft({ ...draft, parts: nextParts });
    if (selectedPart?.id === removed.id) {
      setSelectedPartId(nextParts[Math.min(index, nextParts.length - 1)]?.id);
    }
  };
  const exitWorkspace = () => {
    setDetailsOpen(false);
    window.dispatchEvent(new Event("rackforge:close-graph-workspace"));
  };
  const workspaceDetails = (mobile = false) => (
    <SongWorkspaceDetails
      partName={selectedPart?.name ?? "Song Part"}
      dirty={dirty}
      isNew={isNew}
      pending={pending}
      previewStatus={partPreview.status}
      instrumentCount={partPreview.voiceCount}
      onPartName={(name) => {
        if (selectedPartIndex < 0) return;
        updatePart(selectedPartIndex, (part) => ({ ...part, name }));
      }}
      onSave={() => void save()}
      onExit={exitWorkspace}
      onDismiss={mobile ? () => setDetailsOpen(false) : undefined}
      mobile={mobile}
    />
  );
  return (
    <form
      className={`performance-form song-editor-form entity-song${immersive ? " immersive" : ""}`}
      onSubmit={(event) => event.preventDefault()}
    >
      <EditorHeader eyebrow={isNew ? "New Song" : "Song configuration"} title={draft.name} dirty={dirty || isNew} pending={pending} onSave={save} onReset={() => { setDraft(original ? clone(original) : newSong(performance)); setBaseRevision(performance.revision); setError(null); }} onDelete={isNew ? undefined : remove} />
      {error && <div className="form-error">{error}</div>}
      {partPreview.error ? <div className="form-error">Part preview: {partPreview.error}</div> : null}
      {partPreview.status !== "idle" ? (
        <div className={`rack-preview-status ${partPreview.status}`} role="status" aria-live="polite">
          {partPreview.status === "applying" ? (
            <><AsyncSpinner label="Applying Song Part preview…" /><span>Applying Song Part preview…</span></>
          ) : (
            <><i /><span>{partPreview.voiceCount} {partPreview.voiceCount === 1 ? "instrument" : "instruments"} active in this Part</span></>
          )}
        </div>
      ) : null}
      <BasicFields name={draft.name} enabled={draft.enabled} onName={(name) => setDraft({ ...draft, name })} onEnabled={(enabled) => setDraft({ ...draft, enabled })} />
      {immersive ? <aside className="song-workspace-details-panel">{workspaceDetails()}</aside> : null}
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
        title="Song Parts"
        detail="Parts are ordered scenes. Every Part owns a graph; all connected instrument and Rack paths can sound together."
        action={(
          <button
            type="button"
            disabled={draft.parts.length >= 64}
            onClick={addPart}
          >
            ＋ Add Part
          </button>
        )}
      >
        <div className="song-part-editor-layout">
          {!immersive ? <aside className="song-part-navigator" aria-label="Song Part order">
            {draft.parts.map((part, index) => (
              <article
                className={`song-part-nav-item entity-song-part${part.id === selectedPart?.id ? " active" : ""}`}
                key={part.id}
              >
                <button
                  type="button"
                  className="song-part-select"
                  onClick={() => setSelectedPartId(part.id)}
                >
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <strong>{part.name}</strong>
                  <small>{part.content ? "Graph" : "Legacy Rack"}</small>
                </button>
                <div className="song-part-order-actions">
                  <button
                    type="button"
                    className="song-part-edit-graph"
                    aria-label={`Edit ${part.name} graph`}
                    onClick={() => {
                      setSelectedPartId(part.id);
                      onEditPart(part);
                    }}
                  >Edit graph</button>
                  <button
                    type="button"
                    aria-label={`Move ${part.name} up`}
                    disabled={index === 0}
                    onClick={() => movePart(index, -1)}
                  >↑</button>
                  <button
                    type="button"
                    aria-label={`Move ${part.name} down`}
                    disabled={index === draft.parts.length - 1}
                    onClick={() => movePart(index, 1)}
                  >↓</button>
                  <button
                    type="button"
                    aria-label={`Remove ${part.name}`}
                    disabled={draft.parts.length === 1}
                    onClick={() => removePart(index)}
                  >×</button>
                </div>
              </article>
            ))}
          </aside> : null}
          {immersive && selectedPart && selectedPartRack ? (
            <section className="song-part-graph-workspace entity-song-part">
              <header className="song-part-graph-header">
                <label>
                  <span>Part name</span>
                  <input
                    value={selectedPart.name}
                    maxLength={64}
                    onChange={(event) => updatePart(
                      selectedPartIndex,
                      (part) => ({ ...part, name: event.target.value }),
                    )}
                  />
                </label>
                <div>
                  <span>Scene</span>
                  <strong>{selectedPartIndex + 1} / {draft.parts.length}</strong>
                </div>
              </header>
              <Suspense fallback={<div className="rack-graph-loading">Loading graph editor…</div>}>
                <RackGraphEditor
                  rack={selectedPartRack}
                  racks={performance.library.racks}
                  onChange={updatePartRack}
                  canAddInstrument={selectedPartRack.slots.length < 32}
                  onAddInstrument={(position, role) => setPluginPicker({ position, role })}
                  instances={instances}
                  renderPluginSurface={renderPluginSurface}
                  onOverlayChange={handleGraphOverlayChange}
                />
              </Suspense>
              {selectedPartRack.slots.length > 0 ? (
                <div className="song-part-instrument-settings">
                  <span className="card-kicker">Part instruments</span>
                  {selectedPartRack.slots.map((slot, index) => (
                    <SlotEditor
                      key={`${slot.id}:${slot.plugin_id}`}
                      slot={slot}
                      index={index}
                      total={selectedPartRack.slots.length}
                      instances={instances}
                      allowEmpty
                      onChange={(next) => updatePartRack({
                        ...selectedPartRack,
                        slots: selectedPartRack.slots.map((item) => item.id === slot.id ? next : item),
                      })}
                      onMove={(direction) => {
                        const target = index + direction;
                        if (target < 0 || target >= selectedPartRack.slots.length) return;
                        const slots = [...selectedPartRack.slots];
                        [slots[index], slots[target]] = [slots[target], slots[index]];
                        updatePartRack({ ...selectedPartRack, slots });
                      }}
                      onRemove={() => updatePartRack(removeSlotFromRack(selectedPartRack, slot.id))}
                    />
                  ))}
                </div>
              ) : null}
            </section>
          ) : null}
        </div>
      </EditorSection>
      {pluginPicker && selectedPartRack ? (
        <PluginPickerDialog
          instances={instances}
          plugins={plugins}
          role={pluginPicker.role}
          onSelect={(instance) => {
            updatePartRack(
              addSlotToRack(
                selectedPartRack,
                defaultSlot(instance),
                pluginPicker.position,
                rackPluginRole(instance.plugin_id, plugins),
              ),
            );
            setPluginPicker(null);
          }}
          onClose={() => setPluginPicker(null)}
        />
      ) : null}
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
    <form className="performance-form entity-setlist" onSubmit={(event) => event.preventDefault()}>
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
