import { type CSSProperties, useEffect, useRef, useState } from "react";
import { dispatchCommand, subscribeOutputMeter } from "../gateway";
import { METER_FLOOR_DB, amplitudeToMeterDb, meterPercent } from "../meter";
import { type OutputMeterSnapshot } from "../types";

export function MasterOutputMeter() {
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
            <em style={{ bottom: `${meterPercent(holds[channel])}%` }} />
          </i>
        ))}
      </span>
      <span className="master-output-meter-channels" aria-hidden="true">L R</span>
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
