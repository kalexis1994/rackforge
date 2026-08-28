/**
 * The LIVE sequencer surface: a transport strip that is always on the panel,
 * and the sequencer deck that folds out of it.
 *
 * The strip is the machine's clock made visible — PLAY latches, STOP stops,
 * the tempo LCD reads the transport that actually runs the audio, never a
 * local mirror.
 *
 * The deck is tabs: each tab is one lane of the engine — one sequencer, in
 * the player's vocabulary — with its own pattern and its own editing lens.
 * Adding a sequencer asks for a name and a type: DRUM is the fixed-row grid,
 * MELODIC is the step-and-pitch lane of the classic analog sequencers. Under
 * either lens a pattern is the same library entity; LAUNCH plays the draft
 * you are looking at, so editing is auditioning on every tab.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  dispatchPerformanceEdit,
  sendSequencerCommand,
  subscribeSequencerStatus,
} from "./gateway";
import {
  MAX_SEQUENCER_LANES,
  MELODIC_VELOCITIES,
  STEP_ROWS,
  STEP_TICKS,
  TICKS_PER_BEAT,
  clearMelodicStep,
  cycleMelodicTie,
  cycleMelodicVelocity,
  emptyPattern,
  hasStep,
  melodicStepNote,
  noteName,
  setMelodicStep,
  stepCount,
  tapTempo,
  toggleStep,
  transposeMelodicStep,
  type SequencerStatus,
} from "./sequencer";
import type { PatternDefinition, PerformanceSnapshot } from "./types";

const OPEN_TABS_KEY = "rackforge.sequencer-tabs.v1";

interface SequencerTab {
  lane: number;
  draft: PatternDefinition;
  dirty: boolean;
}

function useSequencerStatus(): SequencerStatus | null {
  const [status, setStatus] = useState<SequencerStatus | null>(null);
  useEffect(() => subscribeSequencerStatus(setStatus), []);
  return status;
}

export function SequencerStrip({
  performance,
  surface,
}: {
  performance: PerformanceSnapshot;
  surface: "perform" | "configure";
}) {
  const status = useSequencerStatus();
  const [open, setOpen] = useState(false);
  const taps = useRef<number[]>([]);

  const tap = useCallback(() => {
    const now = performance_now_seconds();
    taps.current = [...taps.current.slice(-4), now];
    const bpm = tapTempo(taps.current);
    if (bpm !== null) {
      sendSequencerCommand({ kind: "set_tempo", bpm });
    }
  }, []);

  const nudgeTempo = useCallback(
    (delta: number) => {
      if (!status) return;
      sendSequencerCommand({ kind: "set_tempo", bpm: status.tempo_bpm + delta });
    },
    [status],
  );

  const running = status?.running ?? false;
  return (
    <section className="sequencer-shell" aria-label="Sequencer">
      <span className="live-zone-legend">SEQUENCER</span>
      <div className="sequencer-strip">
        <span className="seq-legend">SEQ</span>
        <div className="seq-keys" role="group" aria-label="Transport">
          <button
            className={`seq-key seq-lamp-key${running ? " engaged" : ""}`}
            aria-pressed={running}
            onClick={() =>
              sendSequencerCommand({ kind: running ? "transport_stop" : "transport_start" })
            }
          >
            PLAY
          </button>
          <button
            className="seq-key"
            onClick={() => sendSequencerCommand({ kind: "transport_stop" })}
          >
            STOP
          </button>
          <button
            className="seq-key seq-panic"
            title="All notes off, all lanes cleared"
            onClick={() => sendSequencerCommand({ kind: "transport_panic" })}
          >
            PANIC
          </button>
        </div>
        <div className="seq-lcd" role="status" aria-label="Transport position">
          <span className={`seq-beat-lamp${running && (status?.beat_phase ?? 1) < 0.22 ? " lit" : ""}`} />
          <span className="seq-lcd-position">
            {status ? `${status.bar}.${status.beat_in_bar}` : "-.-"}
          </span>
          <span className="seq-lcd-signature">
            {status ? `${status.beats_per_bar}/${status.beat_unit}` : "-/-"}
          </span>
        </div>
        <div className="seq-tempo" role="group" aria-label="Tempo">
          <button className="seq-key seq-key-narrow" onClick={() => nudgeTempo(-1)} disabled={!status}>
            −
          </button>
          <span className="seq-lcd seq-lcd-tempo">
            {status ? status.tempo_bpm.toFixed(1) : "---.-"}
            <small>BPM</small>
          </span>
          <button className="seq-key seq-key-narrow" onClick={() => nudgeTempo(1)} disabled={!status}>
            +
          </button>
          <button className="seq-key" onClick={tap}>
            TAP
          </button>
        </div>
        <button
          className={`seq-key seq-lamp-key seq-open-key${open ? " engaged" : ""}`}
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          SEQUENCERS
        </button>
      </div>
      {surface === "perform" ? <LanePadDeck status={status} /> : null}
      {open ? <SequencerDeck performance={performance} status={status} /> : null}
    </section>
  );
}

/**
 * The PERFORM pad deck: one pad per lane, playable without opening the
 * deck. The machine remembers what each lane holds, so a pad press is
 * `launch_lane` — no document travels. Press again while sounding to stop
 * at the bar; press a stopping pad to change your mind before it arrives.
 */
function LanePadDeck({ status }: { status: SequencerStatus | null }) {
  const lanes = status?.lanes ?? [];
  const press = useCallback(
    (lane: number, state: { playing: boolean; queued: boolean; stopping: boolean }) => {
      if (state.queued) {
        sendSequencerCommand({ kind: "stop_lane", lane, quantize: "now" });
        return;
      }
      if (state.playing && !state.stopping) {
        sendSequencerCommand({ kind: "stop_lane", lane, quantize: "next_bar" });
        return;
      }
      sendSequencerCommand({ kind: "launch_lane", lane, quantize: "next_bar" });
      if (!status?.running) {
        sendSequencerCommand({ kind: "transport_start" });
      }
    },
    [status?.running],
  );
  return (
    <div className="seq-pads" role="group" aria-label="Lane pads">
      {Array.from({ length: MAX_SEQUENCER_LANES }, (_, lane) => {
        const state = lanes[lane];
        const loaded = Boolean(state?.pattern_name);
        const playing = state?.playing ?? false;
        const queued = state?.queued ?? false;
        const stopping = state?.stopping ?? false;
        const face = playing
          ? stopping
            ? "stopping"
            : "playing"
          : queued
          ? "queued"
          : loaded
          ? "loaded"
          : "empty";
        return (
          <button
            key={lane}
            className={`seq-pad ${face}${state?.muted ? " muted" : ""}`}
            disabled={!loaded}
            aria-pressed={playing || queued}
            onClick={() => press(lane, { playing, queued, stopping })}
          >
            <span className="seq-pad-lane">{lane + 1}</span>
            <span className="seq-pad-name">{state?.pattern_name ?? "empty"}</span>
            <span className="seq-pad-state">
              {playing
                ? stopping
                  ? "STOPPING"
                  : "PLAYING"
                : queued
                ? "QUEUED"
                : loaded
                ? "READY"
                : " "}
            </span>
          </button>
        );
      })}
    </div>
  );
}

function performance_now_seconds(): number {
  return (typeof window !== "undefined" ? window.performance.now() : Date.now()) / 1000;
}

function restoreTabs(patterns: PatternDefinition[]): SequencerTab[] {
  try {
    const raw = JSON.parse(localStorage.getItem(OPEN_TABS_KEY) ?? "[]") as {
      lane: number;
      pattern_id: string;
    }[];
    return raw
      .filter((entry) => Number.isInteger(entry.lane) && entry.lane >= 0 && entry.lane < 8)
      .flatMap((entry) => {
        const pattern = patterns.find((candidate) => candidate.id === entry.pattern_id);
        return pattern ? [{ lane: entry.lane, draft: pattern, dirty: false }] : [];
      });
  } catch {
    return [];
  }
}

function persistTabs(tabs: SequencerTab[]) {
  try {
    localStorage.setItem(
      OPEN_TABS_KEY,
      JSON.stringify(tabs.map((tab) => ({ lane: tab.lane, pattern_id: tab.draft.id }))),
    );
  } catch {
    // A browser that blocks storage just loses tab restoration.
  }
}

/** The tabbed deck: one tab per sequencer, one sequencer per engine lane. */
function SequencerDeck({
  performance,
  status,
}: {
  performance: PerformanceSnapshot;
  status: SequencerStatus | null;
}) {
  const patterns = performance.library.patterns ?? [];
  const beatsPerBar = status?.beats_per_bar ?? 4;
  const [tabs, setTabs] = useState<SequencerTab[]>(() => restoreTabs(patterns));
  const [activeLane, setActiveLane] = useState<number | null>(tabs[0]?.lane ?? null);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => persistTabs(tabs), [tabs]);

  // A clean tab follows its library pattern when someone else edits it.
  useEffect(() => {
    setTabs((current) =>
      current.map((tab) => {
        if (tab.dirty) return tab;
        const library = patterns.find((pattern) => pattern.id === tab.draft.id);
        return library && library !== tab.draft ? { ...tab, draft: library } : tab;
      }),
    );
  }, [patterns]);

  const updateTab = useCallback((lane: number, update: (tab: SequencerTab) => SequencerTab) => {
    setTabs((current) => current.map((tab) => (tab.lane === lane ? update(tab) : tab)));
  }, []);

  const freeLane = Array.from({ length: MAX_SEQUENCER_LANES }, (_, lane) => lane).find(
    (lane) => !tabs.some((tab) => tab.lane === lane),
  );

  const addSequencer = useCallback(
    (name: string, view: "drum" | "melodic") => {
      if (freeLane === undefined) return;
      const draft = emptyPattern(name, 1, beatsPerBar, view);
      setTabs((current) => [...current, { lane: freeLane, draft, dirty: true }]);
      setActiveLane(freeLane);
      setAdding(false);
    },
    [freeLane, beatsPerBar],
  );

  const closeTab = useCallback(
    (lane: number) => {
      sendSequencerCommand({ kind: "stop_lane", lane, quantize: "now" });
      setTabs((current) => {
        const next = current.filter((tab) => tab.lane !== lane);
        setActiveLane((active) => (active === lane ? next[0]?.lane ?? null : active));
        return next;
      });
    },
    [],
  );

  const active = tabs.find((tab) => tab.lane === activeLane) ?? tabs[0] ?? null;

  return (
    <div className="sequencer-deck">
      <div className="seq-tabs" role="tablist" aria-label="Sequencers">
        {tabs.map((tab) => {
          const state = status?.lanes[tab.lane];
          const face = state?.playing ? (state.stopping ? "stopping" : "playing") : state?.queued ? "queued" : "";
          return (
            <button
              key={tab.lane}
              role="tab"
              aria-selected={tab.lane === active?.lane}
              className={`seq-tab ${face}${tab.lane === active?.lane ? " active" : ""}`}
              onClick={() => setActiveLane(tab.lane)}
            >
              <span className="seq-tab-lane">{tab.lane + 1}</span>
              <span className="seq-tab-name">
                {tab.draft.name}
                {tab.dirty ? " *" : ""}
              </span>
              <span className="seq-tab-kind">{tab.draft.view === "melodic" ? "MEL" : "DRM"}</span>
            </button>
          );
        })}
        <button
          className="seq-key seq-tab-add"
          disabled={freeLane === undefined}
          title={freeLane === undefined ? "All 8 lanes are in use" : "Add a sequencer"}
          onClick={() => setAdding((value) => !value)}
        >
          ＋
        </button>
      </div>
      {adding ? (
        <AddSequencerForm
          nextLane={freeLane}
          onCreate={addSequencer}
          onCancel={() => setAdding(false)}
        />
      ) : null}
      {!active ? (
        <div className="seq-editor seq-editor-empty">
          <p>No sequencers yet. ＋ adds one — drum or melodic — on its own lane.</p>
        </div>
      ) : (
        <SequencerTabEditor
          key={active.lane}
          tab={active}
          patterns={patterns}
          revision={performance.revision}
          beatsPerBar={beatsPerBar}
          status={status}
          busy={busy}
          setBusy={setBusy}
          onChange={(update) => updateTab(active.lane, update)}
          onClose={() => closeTab(active.lane)}
        />
      )}
    </div>
  );
}

/** Name and type, then a lane: everything a new sequencer needs. */
function AddSequencerForm({
  nextLane,
  onCreate,
  onCancel,
}: {
  nextLane: number | undefined;
  onCreate: (name: string, view: "drum" | "melodic") => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [view, setView] = useState<"drum" | "melodic">("drum");
  const create = () => {
    const trimmed = name.trim();
    onCreate(trimmed.length > 0 ? trimmed : view === "drum" ? "Drums" : "Melody", view);
  };
  return (
    <div className="seq-add-form" role="form" aria-label="New sequencer">
      <label className="seq-add-name">
        <span>NAME</span>
        <input
          className="seq-name-input"
          value={name}
          maxLength={48}
          placeholder={view === "drum" ? "Drums" : "Melody"}
          autoFocus
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") create();
          }}
        />
      </label>
      <div className="seq-keys" role="group" aria-label="Sequencer type">
        <button
          className={`seq-key seq-lamp-key${view === "drum" ? " engaged" : ""}`}
          aria-pressed={view === "drum"}
          onClick={() => setView("drum")}
        >
          DRUM
        </button>
        <button
          className={`seq-key seq-lamp-key${view === "melodic" ? " engaged" : ""}`}
          aria-pressed={view === "melodic"}
          onClick={() => setView("melodic")}
        >
          MELODIC
        </button>
      </div>
      <span className="seq-inline-label">
        {nextLane !== undefined ? `LANE ${nextLane + 1} · CH ${nextLane + 1}` : "DECK FULL"}
      </span>
      <div className="seq-keys">
        <button className="seq-key seq-launch" disabled={nextLane === undefined} onClick={create}>
          CREATE
        </button>
        <button className="seq-key" onClick={onCancel}>
          CANCEL
        </button>
      </div>
    </div>
  );
}

function SequencerTabEditor({
  tab,
  patterns,
  revision,
  beatsPerBar,
  status,
  busy,
  setBusy,
  onChange,
  onClose,
}: {
  tab: SequencerTab;
  patterns: PatternDefinition[];
  revision: string;
  beatsPerBar: number;
  status: SequencerStatus | null;
  busy: boolean;
  setBusy: (value: boolean) => void;
  onChange: (update: (tab: SequencerTab) => SequencerTab) => void;
  onClose: () => void;
}) {
  const { lane, draft, dirty } = tab;
  const laneState = status?.lanes[lane];
  const inLibrary = patterns.some((pattern) => pattern.id === draft.id);
  const bars = Math.max(1, Math.round(draft.length_ticks / (beatsPerBar * TICKS_PER_BEAT)));

  const edit = useCallback(
    (next: PatternDefinition) => {
      onChange((current) => ({ ...current, draft: next, dirty: true }));
    },
    [onChange],
  );

  const setBars = useCallback(
    (value: number) => {
      const length = value * beatsPerBar * TICKS_PER_BEAT;
      edit({
        ...draft,
        length_ticks: length,
        notes: draft.notes.filter((note) => note.tick < length),
      });
    },
    [draft, beatsPerBar, edit],
  );

  const save = useCallback(() => {
    if (busy) return;
    setBusy(true);
    dispatchPerformanceEdit(revision, { kind: "put_pattern", pattern: draft })
      .then(() => onChange((current) => ({ ...current, dirty: false })))
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [busy, setBusy, revision, draft, onChange]);

  const remove = useCallback(() => {
    if (busy || !inLibrary) return;
    setBusy(true);
    dispatchPerformanceEdit(revision, { kind: "delete_pattern", pattern_id: draft.id })
      .then(() => onClose())
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [busy, setBusy, inLibrary, revision, draft.id, onClose]);

  const launch = useCallback(() => {
    sendSequencerCommand({ kind: "queue_pattern", lane, pattern: draft, quantize: "next_bar" });
    if (!status?.running) {
      sendSequencerCommand({ kind: "transport_start" });
    }
  }, [lane, draft, status?.running]);

  return (
    <div className="seq-editor">
      <header className="seq-editor-header">
        <input
          className="seq-name-input"
          value={draft.name}
          maxLength={48}
          onChange={(event) => edit({ ...draft, name: event.target.value })}
          aria-label="Sequencer name"
        />
        <div className="seq-keys" role="group" aria-label="Pattern length">
          {[1, 2, 4].map((value) => (
            <button
              key={value}
              className={`seq-key seq-key-narrow${bars === value ? " engaged" : ""}`}
              aria-pressed={bars === value}
              onClick={() => setBars(value)}
            >
              {value}
            </button>
          ))}
          <span className="seq-inline-label">BARS</span>
        </div>
        <select
          className="seq-load-select"
          value=""
          aria-label="Load a pattern from the library"
          onChange={(event) => {
            const pattern = patterns.find((candidate) => candidate.id === event.target.value);
            if (pattern) {
              onChange((current) => ({ ...current, draft: pattern, dirty: false }));
            }
          }}
        >
          <option value="">LOAD…</option>
          {patterns.map((pattern) => (
            <option key={pattern.id} value={pattern.id}>
              {pattern.name}
            </option>
          ))}
        </select>
        <div className="seq-keys">
          <button className="seq-key" onClick={save} disabled={!dirty || busy}>
            SAVE
          </button>
          <button className="seq-key" onClick={remove} disabled={busy || !inLibrary}>
            DELETE
          </button>
          <button className="seq-key" onClick={onClose} title="Close this sequencer's tab">
            CLOSE
          </button>
        </div>
      </header>
      {draft.view === "melodic" ? (
        <MelodicLane pattern={draft} onEdit={edit} />
      ) : (
        <DrumGrid pattern={draft} onEdit={edit} />
      )}
      <footer className="seq-lanes" aria-label="Lane controls">
        <span className="seq-inline-label">{`LANE ${lane + 1} · CH ${lane + 1}`}</span>
        <div className="seq-keys">
          <button className="seq-key seq-launch" onClick={launch}>
            LAUNCH
          </button>
          <button
            className="seq-key"
            onClick={() => sendSequencerCommand({ kind: "stop_lane", lane, quantize: "next_bar" })}
          >
            STOP LANE
          </button>
          <button
            className={`seq-key seq-lamp-key${laneState?.muted ? " engaged" : ""}`}
            aria-pressed={laneState?.muted ?? false}
            onClick={() =>
              sendSequencerCommand({
                kind: "set_lane_muted",
                lane,
                muted: !(laneState?.muted ?? false),
              })
            }
          >
            MUTE
          </button>
        </div>
        <span className="seq-lane-state-word">
          {laneState?.playing
            ? laneState.stopping
              ? "STOPPING"
              : "PLAYING"
            : laneState?.queued
            ? "QUEUED"
            : laneState?.pattern_name
            ? "READY"
            : ""}
        </span>
      </footer>
    </div>
  );
}

/** The drum lens: fixed GM rows, one cell per 16th. */
function DrumGrid({
  pattern,
  onEdit,
}: {
  pattern: PatternDefinition;
  onEdit: (next: PatternDefinition) => void;
}) {
  const steps = stepCount(pattern);
  const stepsPerBeat = TICKS_PER_BEAT / STEP_TICKS;
  return (
    <div className="seq-grid-scroll">
      <div
        className="seq-grid"
        role="grid"
        aria-label={`Step grid, ${steps} steps`}
        style={{ ["--seq-steps" as string]: steps }}
      >
        {STEP_ROWS.map((row) => (
          <div className="seq-grid-row" role="row" key={row.key}>
            <span className="seq-row-label" role="rowheader">
              {row.label}
            </span>
            {Array.from({ length: steps }, (_, step) => {
              const on = hasStep(pattern, row.key, step);
              return (
                <button
                  key={step}
                  role="gridcell"
                  aria-selected={on}
                  className={`seq-cell${on ? " on" : ""}${
                    step % stepsPerBeat === 0 ? " beat" : ""
                  }`}
                  onClick={() => onEdit(toggleStep(pattern, row.key, step))}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

const DEFAULT_MELODIC_KEY = 48; // C2, where basses live

/**
 * The melodic lens: one voice, one step lane, edited the way the classic
 * analog sequencers were played — select a step, then turn its pitch,
 * octave, tie and accent with the cluster. Clicking an empty step plants
 * the last pitch you used, so walking a bassline in is a drum-roll of
 * clicks and a few nudges.
 */
function MelodicLane({
  pattern,
  onEdit,
}: {
  pattern: PatternDefinition;
  onEdit: (next: PatternDefinition) => void;
}) {
  const steps = stepCount(pattern);
  const stepsPerBeat = TICKS_PER_BEAT / STEP_TICKS;
  const [selected, setSelected] = useState(0);
  const lastKey = useRef(DEFAULT_MELODIC_KEY);
  const note = melodicStepNote(pattern, selected);
  if (note) lastKey.current = note.key;

  const press = (step: number) => {
    setSelected(step);
    if (!melodicStepNote(pattern, step)) {
      onEdit(setMelodicStep(pattern, step, lastKey.current));
    }
  };

  const velocityLabel = (velocity: number) =>
    velocity >= MELODIC_VELOCITIES[2] ? "ACC" : velocity <= MELODIC_VELOCITIES[0] ? "SOFT" : "MED";

  return (
    <div className="melodic-lane">
      <div className="seq-grid-scroll">
        <div className="melodic-steps" role="listbox" aria-label={`Melodic lane, ${steps} steps`}>
          {Array.from({ length: steps }, (_, step) => {
            const stepNote = melodicStepNote(pattern, step);
            const ties = stepNote ? Math.round(stepNote.duration_ticks / STEP_TICKS) : 1;
            return (
              <button
                key={step}
                role="option"
                aria-selected={step === selected}
                className={[
                  "melodic-step",
                  stepNote ? "on" : "",
                  step === selected ? "selected" : "",
                  step % stepsPerBeat === 0 ? "beat" : "",
                  stepNote && stepNote.velocity >= MELODIC_VELOCITIES[2] ? "accent" : "",
                  stepNote && stepNote.velocity <= MELODIC_VELOCITIES[0] ? "soft" : "",
                ].join(" ").replace(/\s+/g, " ").trim()}
                onClick={() => press(step)}
              >
                <span className="melodic-step-note">{stepNote ? noteName(stepNote.key) : "·"}</span>
                <span className={`melodic-step-tie tie-${ties}`} aria-hidden="true" />
              </button>
            );
          })}
        </div>
      </div>
      <div className="melodic-cluster" role="group" aria-label="Step editing">
        <span className="seq-lcd melodic-readout">
          {`STEP ${selected + 1}`}
          <strong>{note ? noteName(note.key) : "—"}</strong>
          {note ? (
            <small>{`×${Math.round(note.duration_ticks / STEP_TICKS)} · ${velocityLabel(note.velocity)}`}</small>
          ) : (
            <small>EMPTY</small>
          )}
        </span>
        <div className="seq-keys" role="group" aria-label="Pitch">
          <button className="seq-key seq-key-narrow" disabled={!note} onClick={() => onEdit(transposeMelodicStep(pattern, selected, -1))}>
            −
          </button>
          <button className="seq-key seq-key-narrow" disabled={!note} onClick={() => onEdit(transposeMelodicStep(pattern, selected, 1))}>
            +
          </button>
          <button className="seq-key" disabled={!note} onClick={() => onEdit(transposeMelodicStep(pattern, selected, -12))}>
            OCT−
          </button>
          <button className="seq-key" disabled={!note} onClick={() => onEdit(transposeMelodicStep(pattern, selected, 12))}>
            OCT+
          </button>
        </div>
        <div className="seq-keys" role="group" aria-label="Shape">
          <button className="seq-key" disabled={!note} onClick={() => onEdit(cycleMelodicTie(pattern, selected))}>
            TIE
          </button>
          <button className="seq-key" disabled={!note} onClick={() => onEdit(cycleMelodicVelocity(pattern, selected))}>
            VEL
          </button>
          <button className="seq-key" disabled={!note} onClick={() => onEdit(clearMelodicStep(pattern, selected))}>
            CLEAR
          </button>
        </div>
      </div>
    </div>
  );
}
