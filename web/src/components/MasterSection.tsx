import { type CSSProperties, useEffect, useRef, useState } from "react";
import {
  dispatchCommand,
  saveOutputCapture,
  subscribeAudioHealth,
  subscribeOutputMeter,
} from "../gateway";
import { METER_FLOOR_DB, amplitudeToMeterDb, meterPercent } from "../meter";
import { type AudioHealthSnapshot, type OutputMeterSnapshot } from "../types";

/// The audio callback's cost and what it has dropped, beside the meter that
/// is already being watched.
///
/// A dropout that appears at random needs a witness that is always looking,
/// and this telemetry only ever reached stdout -- which a windowed
/// application does not have. The load says whether the fault is ours; the
/// two counters say whose it is if the load is low. They read "0" rather
/// than disappearing, because a counter that hides when it is zero cannot be
/// told apart from one that stopped being read.
export function AudioHealthReadout() {
  const [health, setHealth] = useState<AudioHealthSnapshot | null>(null);
  const [capture, setCapture] = useState<{ state: "saving" | "saved" | "failed"; detail: string } | null>(null);

  useEffect(() => subscribeAudioHealth(setHealth), []);

  // The mark returns to its resting state after a few seconds, so the next
  // click can be saved without wondering whether the last one was.
  useEffect(() => {
    if (!capture || capture.state === "saving") return;
    const timer = window.setTimeout(() => setCapture(null), 6_000);
    return () => window.clearTimeout(timer);
  }, [capture]);

  const saveCapture = async () => {
    setCapture({ state: "saving", detail: "Saving…" });
    try {
      const saved = await saveOutputCapture();
      setCapture({
        state: "saved",
        detail: `Saved ${Math.round(saved.seconds)} s and ${saved.midi_messages} MIDI messages to ${saved.path}`,
      });
    } catch (error) {
      setCapture({
        state: "failed",
        detail: error instanceof Error ? error.message : "The capture could not be saved.",
      });
    }
  };

  if (!health) {
    return null;
  }
  const load = Math.round(health.load_percent);
  const peak = Math.round(health.peak_percent);
  // Everything the host can see that is heard as a click: a callback that
  // took too long, one the driver called too late, a block that went out as
  // silence, audio the driver says it lost on its own side, an input that
  // slipped against the output, and whatever the stream reported as failed.
  const driverDropouts = health.driver_overloads + health.driver_resyncs
    + health.driver_skipped_buffers;
  // Late MIDI is not lost audio, and it is heard the same way from the
  // keyboard: a note that sounds late, a release that holds a key down.
  const midiLate = health.midi_late_driver + health.midi_late_queue;
  const lost = health.overruns + health.late_callbacks + health.silenced_blocks
    + driverDropouts + health.capture_glitches + health.stream_errors + midiLate;
  // The colour marks what is happening now. The total on the right only
  // grows, so colouring by it would leave the readout red for the rest of
  // the session after a single glitch and say nothing afterwards.
  const failingNow = health.recent_overruns > 0
    || health.recent_late_callbacks > 0
    || health.recent_silenced_blocks > 0
    || health.recent_driver_dropouts > 0
    || health.recent_capture_glitches > 0
    || health.recent_midi_late > 0;
  const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? "" : "s"}`;
  const now: string[] = [];
  if (health.recent_overruns > 0) {
    now.push(`${plural(health.recent_overruns, "overrun")} averaging `
      + `${Math.round(health.overrun_average_percent)}% of budget on `
      + `${Math.round(health.overrun_average_frames)}-frame blocks`);
  }
  if (health.recent_late_callbacks > 0) {
    now.push(`${plural(health.recent_late_callbacks, "late callback")}, widest gap `
      + `${Math.round(health.worst_gap_percent)}% of a period`);
  }
  if (health.recent_silenced_blocks > 0) {
    now.push(`${plural(health.recent_silenced_blocks, "block")} sent as silence`);
  }
  if (health.recent_driver_dropouts > 0) {
    now.push(`${plural(health.recent_driver_dropouts, "dropout")} reported by the driver`);
  }
  if (health.recent_capture_glitches > 0) {
    now.push(`${plural(health.recent_capture_glitches, "input slip")}`);
  }
  if (health.recent_midi_late > 0) {
    now.push(`${plural(health.recent_midi_late, "late MIDI message")}, held up to `
      + `${Math.round(health.worst_midi_driver_delay_ms)} ms before RackForge and `
      + `${Math.round(health.worst_midi_queue_delay_ms)} ms inside it`);
  }
  const title = `Audio callback load ${load}%, peak ${peak}% of a `
    + `${Math.round(health.block_frames)}-frame block; widest gap between callbacks `
    + `${Math.round(health.worst_gap_percent)}% of a period. `
    + `Since the stream opened: ${plural(health.overruns, "overrun")}, `
    + `${plural(health.late_callbacks, "late callback")}, `
    + `${plural(health.silenced_blocks, "silenced block")}, `
    + `${plural(driverDropouts, "driver-reported dropout")} (overload `
    + `${health.driver_overloads}, resync ${health.driver_resyncs}, skipped buffers `
    + `${health.driver_skipped_buffers}), `
    + `${plural(health.capture_glitches, "input slip")}, `
    + `${plural(midiLate, "late MIDI message")} (held before RackForge `
    + `${health.midi_late_driver}, inside it ${health.midi_late_queue}), `
    + `${plural(health.stream_errors, "driver error")}, `
    + `${plural(health.midi_dropped, "MIDI event")} dropped.`
    + (now.length > 0 ? ` Now: ${now.join("; ")}.` : " Nothing lost since the last reading.");
  return (
    <div
      className={`audio-health-readout${failingNow ? " audio-health-readout-lost" : ""}`}
      title={title}
      aria-label={title}
    >
      <span className="audio-health-load">{load}%</span>
      <span className="audio-health-lost" aria-hidden="true">{lost}</span>
      {/* A click heard once, with every counter at zero, leaves only the
          audio as a witness. This keeps it: the last fifteen seconds of
          what went to the device and the MIDI that played them. */}
      <button
        type="button"
        className={`audio-health-capture${capture ? ` audio-health-capture-${capture.state}` : ""}`}
        title={capture?.detail
          ?? "Heard a click? Press within fifteen seconds to save the output and the MIDI that played it."}
        aria-label={capture?.detail ?? "Save the last fifteen seconds of output"}
        disabled={capture?.state === "saving"}
        onClick={() => void saveCapture()}
      >
        {capture?.state === "saved" ? "✓" : capture?.state === "failed" ? "!" : "●"}
      </button>
    </div>
  );
}

/**
 * The master output meter. Given `toggle`, it is also the switch between
 * the top bar's two faces on a narrow screen -- what is playing, or the
 * master volume and pan -- which do not fit side by side there. The switch
 * is a transparent key laid over the meter rather than a key around it:
 * the health readout below the bars has a key of its own, and a key may
 * not hold another.
 */
export function MasterOutputMeter({
  toggle,
}: {
  toggle?: { expanded: boolean; onToggle: () => void };
} = {}) {
  const [levels, setLevels] = useState<[number, number]>([METER_FLOOR_DB, METER_FLOOR_DB]);
  const [holds, setHolds] = useState<[number, number]>([METER_FLOOR_DB, METER_FLOOR_DB]);
  const holdUntil = useRef<[number, number]>([0, 0]);

  useEffect(() => subscribeOutputMeter((meter: OutputMeterSnapshot) => {
    const incoming = [
      amplitudeToMeterDb(meter.left_peak),
      amplitudeToMeterDb(meter.right_peak),
    ] as const;
    const now = performance.now();
    setLevels((previous) => [
      Math.max(incoming[0], previous[0] - 2.4),
      Math.max(incoming[1], previous[1] - 2.4),
    ]);
    setHolds((previous) => previous.map((held, channel) => {
      const next = incoming[channel];
      if (next >= held) {
        holdUntil.current[channel] = now + 800;
        return next;
      }
      return now < holdUntil.current[channel]
        ? held
        : Math.max(next, held - 1.2);
    }) as [number, number]);
  }), []);

  const maximum = Math.max(...levels);
  const readable = maximum <= METER_FLOOR_DB
    ? "silent"
    : `${maximum.toFixed(1)} dBFS${maximum >= 0 ? ", clipping" : ""}`;
  return (
    <div className="master-output-meter" role="meter" aria-label={`Master output ${readable}`}>
      <span className="master-output-meter-label">Out</span>
      <span className="master-output-meter-bars" aria-hidden="true">
        {levels.map((level, channel) => (
          <i className="master-output-meter-track" key={channel}>
            <b style={{ height: `${meterPercent(level)}%` }} />
            {/* The peak line only while there is a peak: at the floor it lay
                across the bottom of the slot and squared it off. */}
            {meterPercent(holds[channel]) > 0 ? (
              <em style={{ bottom: `${meterPercent(holds[channel])}%` }} />
            ) : null}
          </i>
        ))}
      </span>
      <span className="master-output-meter-channels" aria-hidden="true">
        <span>L</span>
        <span>R</span>
      </span>
      {toggle ? (
        <button
          type="button"
          className="master-output-meter-toggle"
          aria-expanded={toggle.expanded}
          aria-controls="topbar-mixer"
          aria-label={toggle.expanded ? "Show what is playing" : "Show volume and pan"}
          onClick={toggle.onToggle}
        />
      ) : null}
      <AudioHealthReadout />
    </div>
  );
}

export function MasterLevel({ value }: { value: number }) {
  const [localValue, setLocalValue] = useState(value);
  const [dragging, setDragging] = useState(false);
  const displayedValue = dragging ? localValue : value;
  return (
    <MasterFader
      label="Volume"
      ariaLabel="Master volume"
      minimum={0}
      maximum={1000}
      value={displayedValue}
      output={`${Math.round(displayedValue / 10)}%`}
      onPointerStart={() => {
        setLocalValue(value);
        setDragging(true);
      }}
      onPointerEnd={() => setDragging(false)}
      onChange={(level) => {
        setLocalValue(level);
        dispatchCommand({ type: "set_master_level", level });
      }}
    />
  );
}

export function MasterPan({ value }: { value: number }) {
  const [localValue, setLocalValue] = useState(value);
  const [dragging, setDragging] = useState(false);
  const displayedValue = dragging ? localValue : value;
  const display =
    displayedValue === 0
      ? "C"
      : `${displayedValue < 0 ? "L" : "R"}${Math.round(Math.abs(displayedValue) / 10)}`;
  return (
    <MasterFader
      label="Pan"
      ariaLabel="Master pan"
      minimum={-1000}
      maximum={1000}
      step={10}
      value={displayedValue}
      output={display}
      centered
      onPointerStart={() => {
        setLocalValue(value);
        setDragging(true);
      }}
      onPointerEnd={() => setDragging(false)}
      onDoubleClick={() => {
        setLocalValue(0);
        dispatchCommand({ type: "set_master_pan", pan: 0 });
      }}
      onChange={(nextPan) => {
        const pan = Math.abs(nextPan) <= 70 ? 0 : nextPan;
        setLocalValue(pan);
        dispatchCommand({ type: "set_master_pan", pan });
      }}
    />
  );
}

export function MasterFader({
  label,
  ariaLabel,
  minimum,
  maximum,
  step = 1,
  value,
  output,
  centered = false,
  onPointerStart,
  onPointerEnd,
  onDoubleClick,
  onChange,
}: {
  label: string;
  ariaLabel: string;
  minimum: number;
  maximum: number;
  step?: number;
  value: number;
  output: string;
  centered?: boolean;
  onPointerStart: () => void;
  onPointerEnd: () => void;
  onDoubleClick?: () => void;
  onChange: (value: number) => void;
}) {
  const position = ((value - minimum) / (maximum - minimum)) * 100;
  const start = centered ? Math.min(50, position) : 0;
  const span = centered ? Math.abs(position - 50) : position;
  const style = {
    "--fader-position": `${position}%`,
    "--fader-start": `${start}%`,
    "--fader-span": `${span}%`,
  } as CSSProperties;
  return (
    <label className={`compact-control${centered ? " is-centered pan-control" : ""}`}>
      <span className="compact-control-label">{label}</span>
      <span className="compact-fader" style={style}>
        <i className="compact-fader-rail" aria-hidden="true">
          <b className="compact-fader-fill" />
          {centered ? <b className="compact-fader-center" /> : null}
        </i>
        <input
          type="range"
          aria-label={ariaLabel}
          min={minimum}
          max={maximum}
          step={step}
          value={value}
          onPointerDown={onPointerStart}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerEnd}
          onLostPointerCapture={onPointerEnd}
          onBlur={onPointerEnd}
          onDoubleClick={onDoubleClick}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </span>
      <output>{output}</output>
    </label>
  );
}
