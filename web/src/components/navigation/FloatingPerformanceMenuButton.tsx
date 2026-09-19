import { useCallback, useEffect, useRef, useState } from "react";
import { hostHaptic } from "../../host";
import { Menu } from "lucide-react";

const PERFORMANCE_MENU_POSITION_KEY = "rackforge.performance-menu-position.v1";

function readPerformanceMenuPosition() {
  try {
    const saved = JSON.parse(localStorage.getItem(PERFORMANCE_MENU_POSITION_KEY) ?? "null") as {
      x?: number;
      y?: number;
    } | null;
    return {
      x: Math.min(1, Math.max(0, saved?.x ?? 0)),
      y: Math.min(1, Math.max(0, saved?.y ?? 0)),
    };
  } catch {
    return { x: 0, y: 0 };
  }
}

export function FloatingPerformanceMenuButton({
  menuOpen,
  onOpen,
  showGraphDetails,
}: {
  menuOpen: boolean;
  onOpen: () => void;
  showGraphDetails: boolean;
}) {
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const [position, setPosition] = useState(readPerformanceMenuPosition);
  const positionRef = useRef(position);
  const [dragging, setDragging] = useState(false);
  const gestureRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    grabX: number;
    grabY: number;
    armed: boolean;
    moved: boolean;
    timer: number;
  } | null>(null);
  const suppressClickRef = useRef(false);
  const finishGestureRef = useRef<() => void>(() => undefined);
  const dragCueAnimationRef = useRef<Animation | null>(null);

  const stopDragCue = useCallback(() => {
    dragCueAnimationRef.current?.cancel();
    dragCueAnimationRef.current = null;
  }, []);

  const startDragCue = useCallback(() => {
    stopDragCue();
    const button = buttonRef.current;
    if (!button) return;
    dragCueAnimationRef.current = button.animate(
      [
        {
          color: "var(--acid)",
          backgroundColor: "rgba(4, 17, 26, 0.84)",
          borderColor: "rgba(85, 231, 255, 0.34)",
          boxShadow: "0 8px 26px rgba(0, 3, 7, 0.38)",
          transform: "scale(1)",
        },
        {
          color: "#021016",
          backgroundColor: "var(--acid)",
          borderColor: "#baf7ff",
          boxShadow:
            "0 0 0 7px rgba(85, 231, 255, 0.17), 0 16px 42px rgba(28, 211, 239, 0.42)",
          transform: "scale(1.2)",
        },
      ],
      {
        duration: 180,
        easing: "cubic-bezier(0.2, 0.82, 0.2, 1)",
        fill: "forwards",
      },
    );
  }, [stopDragCue]);

  const moveGesture = useCallback((pointerId: number, clientX: number, clientY: number) => {
    const gesture = gestureRef.current;
    const button = buttonRef.current;
    if (!gesture || gesture.pointerId !== pointerId || !button) return;
    const distance = Math.hypot(clientX - gesture.startX, clientY - gesture.startY);
    if (!gesture.armed) {
      if (distance > 10) {
        gesture.moved = true;
        window.clearTimeout(gesture.timer);
      }
      return;
    }
    gesture.moved = true;
    const shell = button.closest(".app-shell")?.getBoundingClientRect();
    if (!shell) return;
    const availableX = Math.max(1, shell.width - button.offsetWidth - 16);
    const verticalOffset = showGraphDetails ? 110 : 60;
    const availableY = Math.max(1, shell.height - verticalOffset);
    const nextPosition = {
      x: Math.min(1, Math.max(0, (clientX - shell.left - gesture.grabX - 8) / availableX)),
      y: Math.min(1, Math.max(0, (clientY - shell.top - gesture.grabY - 8) / availableY)),
    };
    positionRef.current = nextPosition;
    setPosition(nextPosition);
  }, [showGraphDetails]);

  const finishGesture = useCallback((pointerId?: number) => {
    const gesture = gestureRef.current;
    if (!gesture || (pointerId !== undefined && gesture.pointerId !== pointerId)) return;
    window.clearTimeout(gesture.timer);
    stopDragCue();
    suppressClickRef.current = gesture.armed || gesture.moved;
    if (gesture.armed) {
      localStorage.setItem(PERFORMANCE_MENU_POSITION_KEY, JSON.stringify(positionRef.current));
    }
    gestureRef.current = null;
    setDragging(false);
    const button = buttonRef.current;
    if (button?.hasPointerCapture(gesture.pointerId)) {
      button.releasePointerCapture(gesture.pointerId);
    }
  }, [stopDragCue]);

  useEffect(() => {
    finishGestureRef.current = () => finishGesture();
    return () => {
      finishGestureRef.current = () => undefined;
    };
  }, [finishGesture]);

  useEffect(() => {
    const finishPointer = (event: PointerEvent) => finishGesture(event.pointerId);
    const finishAnyGesture = () => finishGesture();
    const finishWhenHidden = () => {
      if (document.visibilityState !== "visible") finishGesture();
    };
    window.addEventListener("pointerup", finishPointer, true);
    window.addEventListener("pointercancel", finishPointer, true);
    window.addEventListener("touchend", finishAnyGesture, true);
    window.addEventListener("touchcancel", finishAnyGesture, true);
    window.addEventListener("mouseup", finishAnyGesture, true);
    window.addEventListener("rackforge:native-touch-end", finishAnyGesture);
    window.addEventListener("blur", finishAnyGesture);
    document.addEventListener("visibilitychange", finishWhenHidden);
    return () => {
      window.removeEventListener("pointerup", finishPointer, true);
      window.removeEventListener("pointercancel", finishPointer, true);
      window.removeEventListener("touchend", finishAnyGesture, true);
      window.removeEventListener("touchcancel", finishAnyGesture, true);
      window.removeEventListener("mouseup", finishAnyGesture, true);
      window.removeEventListener("rackforge:native-touch-end", finishAnyGesture);
      window.removeEventListener("blur", finishAnyGesture);
      document.removeEventListener("visibilitychange", finishWhenHidden);
      const gesture = gestureRef.current;
      if (gesture) window.clearTimeout(gesture.timer);
      stopDragCue();
      gestureRef.current = null;
    };
  }, [finishGesture, stopDragCue]);

  // One inset for every floating control the editor puts over its canvas, so
  // the menu key, the details key under it and the zoom column on the other
  // side all sit the same distance off the wall. `--rack-float-inset` is
  // declared in the stylesheet; the fallback keeps the old 8px if a host ever
  // renders this without it.
  const floatInset = "var(--rack-float-inset, 8px)";
  const anchorLeft = `calc(${floatInset} + ${position.x * 100}% - ${position.x * 60}px)`;
  const anchorTopOffset = position.y * (showGraphDetails ? 110 : 60);
  const anchorTop = `calc(${floatInset} + ${position.y * 100}% - ${anchorTopOffset}px)`;

  return (
    <>
    <button
      ref={buttonRef}
      className={`performance-menu-button${dragging ? " dragging" : ""}`}
      style={{
        left: anchorLeft,
        top: anchorTop,
      }}
      onPointerDown={(event) => {
        if (!event.isPrimary || event.button !== 0) return;
        const rect = event.currentTarget.getBoundingClientRect();
        const gesture = {
          pointerId: event.pointerId,
          startX: event.clientX,
          startY: event.clientY,
          grabX: event.clientX - rect.left,
          grabY: event.clientY - rect.top,
          armed: false,
          moved: false,
          timer: 0,
        };
        gesture.timer = window.setTimeout(() => {
          if (gestureRef.current !== gesture) return;
          gesture.armed = true;
          setDragging(true);
          startDragCue();
          hostHaptic("tap");
        }, 360);
        gestureRef.current = gesture;
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        moveGesture(event.pointerId, event.clientX, event.clientY);
      }}
      onPointerUp={(event) => finishGesture(event.pointerId)}
      onPointerCancel={(event) => finishGesture(event.pointerId)}
      onTouchEnd={() => finishGesture()}
      onTouchCancel={() => finishGesture()}
      onMouseUp={() => finishGesture()}
      onLostPointerCapture={(event) => finishGesture(event.pointerId)}
      onContextMenu={(event) => event.preventDefault()}
      onClick={() => {
        if (
          suppressClickRef.current ||
          gestureRef.current?.armed ||
          gestureRef.current?.moved
        ) {
          suppressClickRef.current = false;
          finishGesture();
          return;
        }
        onOpen();
      }}
      aria-label="Open RackForge menu. Hold and drag to move this button."
      aria-expanded={menuOpen}
      title="Tap to open · Hold and drag to move"
    >
      <Menu aria-hidden="true" />
    </button>
    </>
  );
}
