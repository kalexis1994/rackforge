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

import {
  IDENTITY_VELOCITY_CURVE,
  sanitiseVelocityCurve,
  velocityCurvePath,
  type VelocityCurve,
} from "../velocityCurve";

const BOX = 100;

type Handle = "low" | "mid" | "high";

export function VelocityCurveEditor({
  curve,
  onChange,
}: {
  curve: VelocityCurve;
  onChange: (curve: VelocityCurve) => void;
}) {
  const sane = sanitiseVelocityCurve(curve);
  const frame = useRef<SVGSVGElement | null>(null);
  const [dragging, setDragging] = useState<Handle | null>(null);
  const titleId = useId();

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
        onChange(sanitiseVelocityCurve({ ...sane, low: at.y }));
        return;
      }
      if (handle === "high") {
        onChange(sanitiseVelocityCurve({ ...sane, high: at.y }));
        return;
      }
      onChange(sanitiseVelocityCurve({ ...sane, mid_input: at.x, mid_output: at.y }));
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
      onChange(sanitiseVelocityCurve({ ...sane, low: sane.low + vertical }));
      return;
    }
    if (handle === "high") {
      onChange(sanitiseVelocityCurve({ ...sane, high: sane.high + vertical }));
      return;
    }
    onChange(
      sanitiseVelocityCurve({
        ...sane,
        mid_input: sane.mid_input + horizontal,
        mid_output: sane.mid_output + vertical,
      }),
    );
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

  return (
    <div className="velocity-curve">
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
        <title id={titleId}>Velocity curve: what the keyboard sent against what is played</title>
        <rect className="velocity-curve-field" x="0" y="0" width={BOX} height={BOX} rx="2" />
        <g className="velocity-curve-grid" aria-hidden="true">
          {[25, 50, 75].map((at) => (
            <line key={`v${at}`} x1={at} y1="0" x2={at} y2={BOX} />
          ))}
          {[25, 50, 75].map((at) => (
            <line key={`h${at}`} x1="0" y1={at} x2={BOX} y2={at} />
          ))}
        </g>
        {/* The keyboard left alone, for the eye to measure the reading against. */}
        <line className="velocity-curve-unity" x1="0" y1={BOX} x2={BOX} y2="0" aria-hidden="true" />
        <path className="velocity-curve-line" d={velocityCurvePath(sane, BOX)} />
        {handles.map((handle) => {
          const at = point(handle.x, handle.y);
          return (
            <circle
              key={handle.id}
              className={`velocity-curve-handle${dragging === handle.id ? " dragging" : ""}`}
              cx={at.cx}
              cy={at.cy}
              r="4.2"
              role="slider"
              tabIndex={0}
              aria-label={handle.label}
              aria-valuemin={0}
              aria-valuemax={127}
              aria-valuenow={handle.y}
              onPointerDown={onPointerDown(handle.id)}
              onKeyDown={onKeyDown(handle.id)}
            />
          );
        })}
      </svg>
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
        <button
          type="button"
          onClick={() => onChange(IDENTITY_VELOCITY_CURVE)}
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
