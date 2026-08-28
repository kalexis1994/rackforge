/**
 * The LIVE sequencer surface: a transport strip that is always on the panel,
 * and the pattern workshop that folds out of it.
 *
 * The strip is the machine's clock made visible — PLAY latches, STOP stops,
 * the tempo LCD reads the transport that actually runs the audio, never a
 * local mirror. The workshop edits patterns as performance-library entities
 * and launches the *draft you are looking at*: editing is auditioning, and
 * what you saved is bit for bit what you will perform.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  dispatchPerformanceEdit,
  sendSequencerCommand,
  subscribeSequencerStatus,
} from "./gateway";
import {
  MAX_SEQUENCER_LANES,
  STEP_ROWS,
  STEP_TICKS,
  TICKS_PER_BEAT,
  emptyPattern,
  hasStep,
  stepCount,
  tapTempo,
  toggleStep,
  type SequencerStatus,
} from "./sequencer";
import type { PatternDefinition, PerformanceSnapshot } from "./types";

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
          PATTERNS
        </button>
      </div>
      {surface === "perform" ? <LanePadDeck status={status} /> : null}
      {open ? <SequencerWorkshop performance={performance} status={status} /> : null}
    </section>
  );
}

/**
 * The PERFORM pad deck: one pad per lane, playable without opening the
 * workshop. The machine remembers what each lane holds, so a pad press is
 * `launch_lane` — no document travels. Press again while sounding to stop
 * at the bar; press a stopping pad to change your mind before it arrives.
 */
function LanePadDeck({ status }: { status: SequencerStatus | null }) {
  const lanes = status?.lanes ?? [];
  const press = useCallback(
    (lane: number, state: { playing: boolean; queued: boolean; stopping: boolean }) => {
      if (state.queued) {
        // A pending launch cancelled before its bar arrives.
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
                : " "}
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

function SequencerWorkshop({
  performance,
  status,
}: {
  performance: PerformanceSnapshot;
  status: SequencerStatus | null;
}) {
  const patterns = performance.library.patterns ?? [];
  const beatsPerBar = status?.beats_per_bar ?? 4;
  const [draft, setDraft] = useState<PatternDefinition | null>(null);
  const [dirty, setDirty] = useState(false);
  const [lane, setLane] = useState(0);
  const [busy, setBusy] = useState(false);

  // The library is the truth for everything not being edited: when the
  // selected pattern changes under a clean draft, follow it.
  const selectedId = draft?.id ?? null;
  const librarySelected = useMemo(
    () => patterns.find((pattern) => pattern.id === selectedId) ?? null,
    [patterns, selectedId],
  );
  useEffect(() => {
    if (!dirty && librarySelected && draft !== librarySelected) {
      setDraft(librarySelected);
    }
  }, [dirty, librarySelected, draft]);

  const select = useCallback((pattern: PatternDefinition) => {
    setDraft(pattern);
    setDirty(false);
  }, []);

  const create = useCallback(() => {
    const name = `Pattern ${patterns.length + 1}`;
    setDraft(emptyPattern(name, 1, beatsPerBar));
    setDirty(true);
  }, [patterns.length, beatsPerBar]);

  const toggle = useCallback((key: number, step: number) => {
    setDraft((current) => (current ? toggleStep(current, key, step) : current));
    setDirty(true);
  }, []);

  const setBars = useCallback(
    (bars: number) => {
      setDraft((current) => {
        if (!current) return current;
        const length = bars * beatsPerBar * TICKS_PER_BEAT;
        return {
          ...current,
          length_ticks: length,
          notes: current.notes.filter((note) => note.tick < length),
        };
      });
      setDirty(true);
    },
    [beatsPerBar],
  );

  const save = useCallback(() => {
    if (!draft || busy) return;
    setBusy(true);
    dispatchPerformanceEdit(performance.revision, { kind: "put_pattern", pattern: draft })
      .then(() => setDirty(false))
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [draft, busy, performance.revision]);

  const remove = useCallback(() => {
    if (!draft || busy) return;
    setBusy(true);
    dispatchPerformanceEdit(performance.revision, {
      kind: "delete_pattern",
      pattern_id: draft.id,
    })
      .then(() => {
        setDraft(null);
        setDirty(false);
      })
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [draft, busy, performance.revision]);

  const launch = useCallback(() => {
    if (!draft) return;
    sendSequencerCommand({
      kind: "queue_pattern",
      lane,
      pattern: draft,
      quantize: "next_bar",
    });
    if (!status?.running) {
      sendSequencerCommand({ kind: "transport_start" });
    }
  }, [draft, lane, status?.running]);

  const laneStatus = status?.lanes ?? [];
  const bars = draft ? Math.max(1, Math.round(draft.length_ticks / (beatsPerBar * TICKS_PER_BEAT))) : 1;
  const steps = draft ? stepCount(draft) : 0;
  const stepsPerBeat = TICKS_PER_BEAT / STEP_TICKS;

  return (
    <div className="sequencer-workshop">
      <aside className="seq-library" aria-label="Pattern library">
        <header>
          <span className="seq-legend">PATTERNS</span>
          <button className="seq-key seq-key-narrow" onClick={create} title="New pattern">
            NEW
          </button>
        </header>
        {patterns.length === 0 && !draft ? (
          <p className="seq-library-empty">No patterns yet. NEW starts one.</p>
        ) : (
          <ul>
            {patterns.map((pattern) => (
              <li key={pattern.id}>
                <button
                  className={`seq-library-entry${pattern.id === selectedId ? " selected" : ""}`}
                  onClick={() => select(pattern)}
                >
                  {pattern.name}
                </button>
              </li>
            ))}
            {draft && !patterns.some((pattern) => pattern.id === draft.id) ? (
              <li>
                <button className="seq-library-entry selected" onClick={() => undefined}>
                  {draft.name} *
                </button>
              </li>
            ) : null}
          </ul>
        )}
      </aside>
      {!draft ? (
        <div className="seq-editor seq-editor-empty">
          <p>Select a pattern or press NEW. LAUNCH plays the draft exactly as you see it.</p>
        </div>
      ) : (
        <div className="seq-editor">
          <header className="seq-editor-header">
            <input
              className="seq-name-input"
              value={draft.name}
              maxLength={48}
              onChange={(event) => {
                setDraft({ ...draft, name: event.target.value });
                setDirty(true);
              }}
              aria-label="Pattern name"
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
            <div className="seq-keys">
              <button className="seq-key" onClick={save} disabled={!dirty || busy}>
                SAVE
              </button>
              <button
                className="seq-key"
                onClick={remove}
                disabled={busy || !patterns.some((pattern) => pattern.id === draft.id)}
              >
                DELETE
              </button>
            </div>
          </header>
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
                    const on = hasStep(draft, row.key, step);
                    return (
                      <button
                        key={step}
                        role="gridcell"
                        aria-selected={on}
                        className={`seq-cell${on ? " on" : ""}${
                          step % stepsPerBeat === 0 ? " beat" : ""
                        }`}
                        onClick={() => toggle(row.key, step)}
                      />
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
          <footer className="seq-lanes" aria-label="Lanes">
            <span className="seq-legend">LANE</span>
            <div className="seq-keys">
              {Array.from({ length: MAX_SEQUENCER_LANES }, (_, index) => {
                const state = laneStatus[index];
                return (
                  <button
                    key={index}
                    className={`seq-key seq-key-narrow seq-lane-key${lane === index ? " engaged" : ""}${
                      state?.playing ? " playing" : state?.queued ? " queued" : ""
                    }`}
                    aria-pressed={lane === index}
                    title={state?.pattern_name ?? undefined}
                    onClick={() => setLane(index)}
                  >
                    {index + 1}
                  </button>
                );
              })}
            </div>
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
                className={`seq-key seq-lamp-key${laneStatus[lane]?.muted ? " engaged" : ""}`}
                aria-pressed={laneStatus[lane]?.muted ?? false}
                onClick={() =>
                  sendSequencerCommand({
                    kind: "set_lane_muted",
                    lane,
                    muted: !(laneStatus[lane]?.muted ?? false),
                  })
                }
              >
                MUTE
              </button>
            </div>
          </footer>
        </div>
      )}
    </div>
  );
}
