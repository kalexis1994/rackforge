import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

interface ScrubNumberFieldProps {
  label: string;
  value: number;
  minimum: number;
  maximum: number;
  step?: number;
  suffix?: string;
  onChange: (value: number) => void;
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(maximum, value));
}

export function ScrubNumberField({
  label,
  value,
  minimum,
  maximum,
  step = 1,
  suffix,
  onChange,
}: ScrubNumberFieldProps) {
  const [draftText, setDraftText] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [handleOffset, setHandleOffset] = useState(0);
  const gestureRef = useRef<{
    pointerId: number;
    x: number;
    time: number;
    remainder: number;
    value: number;
  } | null>(null);

  const emit = (next: number) => {
    const normalized = clamp(
      Math.round(next / step) * step,
      minimum,
      maximum,
    );
    onChange(normalized);
    return normalized;
  };

  const commitText = () => {
    const parsed = Number((draftText ?? String(value)).replace(",", "."));
    if (Number.isFinite(parsed)) emit(parsed);
    setDraftText(null);
  };

  const beginScrub = (event: PointerEvent<HTMLDivElement>) => {
    if (!event.isPrimary || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    gestureRef.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      time: event.timeStamp,
      remainder: 0,
      value,
    };
    setDragging(true);
    setHandleOffset(0);
  };

  const moveScrub = (event: PointerEvent<HTMLDivElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    event.preventDefault();
    const deltaX = event.clientX - gesture.x;
    const elapsed = Math.max(1, event.timeStamp - gesture.time);
    const velocity = Math.abs(deltaX) / elapsed;
    const acceleration = velocity >= 1.5 ? 4 : velocity >= 0.7 ? 2 : 1;
    gesture.remainder += (deltaX / 8) * acceleration;
    const wholeSteps = gesture.remainder < 0
      ? Math.ceil(gesture.remainder)
      : Math.floor(gesture.remainder);
    if (wholeSteps !== 0) {
      gesture.remainder -= wholeSteps;
      gesture.value = emit(gesture.value + wholeSteps * step);
    }
    gesture.x = event.clientX;
    gesture.time = event.timeStamp;
    setHandleOffset(clamp(deltaX, -7, 7));
  };

  const finishScrub = (event: PointerEvent<HTMLDivElement>) => {
    if (gestureRef.current?.pointerId !== event.pointerId) return;
    gestureRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(false);
    setHandleOffset(0);
  };

  const handleKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    const increments: Record<string, number> = {
      ArrowLeft: -step,
      ArrowDown: -step,
      ArrowRight: step,
      ArrowUp: step,
      PageDown: -step * 10,
      PageUp: step * 10,
    };
    if (event.key === "Home") {
      event.preventDefault();
      emit(minimum);
    } else if (event.key === "End") {
      event.preventDefault();
      emit(maximum);
    } else if (increments[event.key] !== undefined) {
      event.preventDefault();
      emit(value + increments[event.key]);
    }
  };

  return (
    <label className="scrub-number-field">
      <span>{label}</span>
      <div className="scrub-number-control">
        <div className="scrub-number-input">
          <input
            type="text"
            inputMode={minimum < 0 ? "text" : "numeric"}
            value={draftText ?? String(value)}
            aria-label={label}
            onFocus={() => setDraftText(String(value))}
            onChange={(event) => setDraftText(event.target.value)}
            onBlur={commitText}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
              if (event.key === "Escape") {
                event.preventDefault();
                setDraftText(null);
              }
            }}
          />
          {suffix ? <small>{suffix}</small> : null}
        </div>
        <div
          className={`scrub-number-slider${dragging ? " dragging" : ""}`}
          role="slider"
          tabIndex={0}
          aria-label={`Adjust ${label}`}
          aria-valuemin={minimum}
          aria-valuemax={maximum}
          aria-valuenow={value}
          aria-valuetext={suffix ? `${value}, ${suffix}` : String(value)}
          title="Drag horizontally to adjust"
          onPointerDown={beginScrub}
          onPointerMove={moveScrub}
          onPointerUp={finishScrub}
          onPointerCancel={finishScrub}
          onKeyDown={handleKeys}
        >
          <i style={{ transform: `translateX(${handleOffset}px)` }} />
        </div>
      </div>
    </label>
  );
}
