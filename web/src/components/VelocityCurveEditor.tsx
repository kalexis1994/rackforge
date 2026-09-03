/**
 * The velocity reading, as a square you can drag.
 *
 * Across is what the keyboard sent, up is what the instruments are told, and
 * the diagonal is the keyboard left alone. Three handles: the floor at the
 * left edge, a bend anywhere inside, the ceiling at the right edge. The
 * endpoints stay on their edges because they ARE the ends of the axis — a
 * floor that could slide inwards would be a second, hidden idea (a dead
 * zone), and this square would then be two controls pretending to be one.
 *
 * The drawing is the host's own arithmetic (`velocityCurve.ts`, mirrored in
 * `crates/rackforge-core/src/velocity_curve.rs`), so what you see here is
 * what the next key you press will do.
 */

import { useCallback, useId, useRef, useState } from "react";

import { useLastStrike } from "../lastStrike";

import {
  IDENTITY_VELOCITY_CURVE,
  bendFraction,
  mapVelocity,
  sanitiseVelocityCurve,
  velocityCurvePath,
  withBendFraction,
  type VelocityCurve,
} from "../velocityCurve";

const BOX = 100;
/** Quarters: enough to read a curve against, few enough to stay quiet. */
const GRID = [25, 50, 75];

type Handle = "low" | "mid" | "high";

export function VelocityCurveEditor({
  curve,
  onChange,
  live = false,
}: {
  curve: VelocityCurve;
  onChange: (curve: VelocityCurve) => void;
  /** Follow the keyboard: ask the host what it last read, and aim at it. */
  live?: boolean;
}) {
  const sane = sanitiseVelocityCurve(curve);
  const frame = useRef<SVGSVGElement | null>(null);
  const [dragging, setDragging] = useState<Handle | null>(null);
  const titleId = useId();
  // The curvature is relative: where the bend sits inside the output span,
  // not what number it happens to be. It is remembered rather than derived on
  // the spot, so an end can be dragged the length of the box and the shape
  // arrives intact -- deriving it from the rounded state at every step let a
  // drift accumulate across a drag. Written only where the bend actually
  // moves, which is in the gestures below.
  const curvature = useRef(bendFraction(sane) ?? 0.5);

  // The keyboard, drawn where it landed. The point on the diagonal is what
  // arrived; the point on the curve is where the reading takes it, which is
  // the same place when the reading is straight. It fades on its own, because
  // a mark that stays is a mark in the way.
  // The fade is the mark's own animation, restarted by keying it on the
  // strike number: a timer here would be a second clock to keep honest, and a
  // piece of state that exists only to say "not any more".
  const strike = useLastStrike(live);

  /** Where a pointer is, in velocity units, whatever the box is scaled to. */
  const readPointer = useCallback((event: { clientX: number; clientY: number }) => {
    const box = frame.current?.getBoundingClientRect();
    if (!box || box.width <= 0 || box.height <= 0) return null;
    const x = ((event.clientX - box.left) / box.width) * 127;
    const y = (1 - (event.clientY - box.top) / box.height) * 127;
    return { x: Math.round(x), y: Math.round(y) };
  }, []);

  const moveHandle = useCallback(
    (handle: Handle, at: { x: number; y: number }) => {
      if (handle === "low") {
        onChange(withBendFraction({ ...sane, low: at.y }, curvature.current));
        return;
      }
      if (handle === "high") {
        onChange(withBendFraction({ ...sane, high: at.y }, curvature.current));
        return;
      }
      const bent = sanitiseVelocityCurve({ ...sane, mid_input: at.x, mid_output: at.y });
      curvature.current = bendFraction(bent) ?? curvature.current;
      onChange(bent);
    },
    [onChange, sane],
  );

  const onPointerDown = (handle: Handle) => (event: React.PointerEvent<SVGElement>) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDragging(handle);
    const at = readPointer(event);
    if (at) moveHandle(handle, at);
  };

  const onPointerMove = (event: React.PointerEvent<SVGElement>) => {
    if (!dragging) return;
    const at = readPointer(event);
    if (at) moveHandle(dragging, at);
  };

  const endDrag = () => setDragging(null);

  /** The keyboard moves a handle by one, or by ten with a shift. */
  const onKeyDown = (handle: Handle) => (event: React.KeyboardEvent<SVGElement>) => {
    const step = event.shiftKey ? 10 : 1;
    const vertical =
      event.key === "ArrowUp" ? step : event.key === "ArrowDown" ? -step : 0;
    const horizontal =
      event.key === "ArrowRight" ? step : event.key === "ArrowLeft" ? -step : 0;
    if (!vertical && !horizontal) return;
    event.preventDefault();
    if (handle === "low") {
      onChange(withBendFraction({ ...sane, low: sane.low + vertical }, curvature.current));
      return;
    }
    if (handle === "high") {
      onChange(withBendFraction({ ...sane, high: sane.high + vertical }, curvature.current));
      return;
    }
    const bent = sanitiseVelocityCurve({
      ...sane,
      mid_input: sane.mid_input + horizontal,
      mid_output: sane.mid_output + vertical,
    });
    curvature.current = bendFraction(bent) ?? curvature.current;
    onChange(bent);
  };

  const point = (x: number, y: number) => ({
    cx: (x / 127) * BOX,
    cy: (1 - y / 127) * BOX,
  });
  const handles: Array<{ id: Handle; label: string; x: number; y: number }> = [
    { id: "low", label: `Floor, ${sane.low}`, x: 0, y: sane.low },
    {
      id: "mid",
      label: `Bend, ${sane.mid_input} in and ${sane.mid_output} out`,
      x: sane.mid_input,
      y: sane.mid_output,
    },
    { id: "high", label: `Ceiling, ${sane.high}`, x: 127, y: sane.high },
  ];

  const line = velocityCurvePath(sane, BOX);
  // The same line closed down the two lower edges: what the reading lets
  // through, as an area, which is what makes a soft or a hard keybed legible
  // at a glance rather than by tracing a stroke.
  const area = `${line} L ${BOX},${BOX} L 0,${BOX} Z`;
  const fillId = `${titleId}-fill`;
  return (
    <div className="velocity-curve">
      <div className={`velocity-curve-frame${dragging ? " dragging" : ""}`}>
        <svg
          ref={frame}
          className="velocity-curve-box"
          viewBox={`0 0 ${BOX} ${BOX}`}
          role="group"
          aria-labelledby={titleId}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerLeave={endDrag}
          onPointerCancel={endDrag}
        >
          <title id={titleId}>
            Velocity curve: what the keyboard sent against what is played
          </title>
          <defs>
            <linearGradient id={fillId} x1="0" y1="1" x2="0" y2="0">
              <stop offset="0%" stopColor="var(--acid)" stopOpacity="0.03" />
              <stop offset="100%" stopColor="var(--acid)" stopOpacity="0.2" />
            </linearGradient>
          </defs>
          <g className="velocity-curve-grid" aria-hidden="true">
            {GRID.map((at) => (
              <line key={`v${at}`} x1={at} y1="0" x2={at} y2={BOX} />
            ))}
            {GRID.map((at) => (
              <line key={`h${at}`} x1="0" y1={at} x2={BOX} y2={at} />
            ))}
          </g>
          {/* The keyboard left alone, for the eye to measure the reading against. */}
          <line
            className="velocity-curve-unity"
            x1="0"
            y1={BOX}
            x2={BOX}
            y2="0"
            aria-hidden="true"
          />
          <path
            className="velocity-curve-area"
            d={area}
            fill={`url(#${fillId})`}
            aria-hidden="true"
          />
          <path className="velocity-curve-line" d={line} />
          {strike && strike.count > 0 ? (
            <g key={strike.count} className="velocity-curve-strike" aria-hidden="true">
              {(() => {
                const arrived = point(strike.velocity, strike.velocity);
                const played = point(strike.velocity, mapVelocity(sane, strike.velocity));
                return (
                  <>
                    {/* From the key that was struck, up to where it was taken,
                        and across to the value the instruments were given. */}
                    <line
                      className="velocity-curve-strike-guide"
                      x1={played.cx}
                      y1={BOX}
                      x2={played.cx}
                      y2={played.cy}
                    />
                    <line
                      className="velocity-curve-strike-guide"
                      x1={played.cx}
                      y1={played.cy}
                      x2="0"
                      y2={played.cy}
                    />
                    <circle
                      className="velocity-curve-strike-arrived"
                      cx={arrived.cx}
                      cy={arrived.cy}
                      r="2"
                    />
                    <circle
                      className="velocity-curve-strike-played"
                      cx={played.cx}
                      cy={played.cy}
                      r="3"
                    />
                  </>
                );
              })()}
            </g>
          ) : null}
          {handles.map((handle) => {
            const at = point(handle.x, handle.y);
            const active = dragging === handle.id;
            return (
              <g key={handle.id} className={`velocity-curve-pin${active ? " dragging" : ""}`}>
                {/* A small mark is a tidy drawing and an unfair target, so the
                    ring under it carries the pointer and the focus. */}
                <circle className="velocity-curve-halo" cx={at.cx} cy={at.cy} r="8" />
                <circle className="velocity-curve-handle" cx={at.cx} cy={at.cy} r="2.4" />
                <circle
                  className="velocity-curve-grab"
                  cx={at.cx}
                  cy={at.cy}
                  r="8"
                  role="slider"
                  tabIndex={0}
                  aria-label={handle.label}
                  aria-valuemin={0}
                  aria-valuemax={127}
                  aria-valuenow={handle.y}
                  onPointerDown={onPointerDown(handle.id)}
                  onKeyDown={onKeyDown(handle.id)}
                />
              </g>
            );
          })}
        </svg>
      </div>
      <div className="velocity-curve-readout">
        <span>
          Floor <strong>{sane.low}</strong>
        </span>
        <span>
          Bend <strong>{sane.mid_input}</strong> → <strong>{sane.mid_output}</strong>
        </span>
        <span>
          Ceiling <strong>{sane.high}</strong>
        </span>
        {strike && strike.count > 0 ? (
          <span key={strike.count} className="velocity-curve-strike-readout">
            Played <strong>{strike.velocity}</strong> → <strong>{mapVelocity(sane, strike.velocity)}</strong>
          </span>
        ) : null}
        <button
          type="button"
          onClick={() => {
            curvature.current = bendFraction(IDENTITY_VELOCITY_CURVE) ?? 0.5;
            onChange(IDENTITY_VELOCITY_CURVE);
          }}
          disabled={
            sane.low === 0 && sane.high === 127 && sane.mid_input === sane.mid_output
          }
        >
          Straight
        </button>
      </div>
    </div>
  );
}
