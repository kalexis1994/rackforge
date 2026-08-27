import { useEffect, useRef } from "react";
import { useLocation } from "react-router";
import { hostHaptic } from "../host";
import {
  beginExperienceSpan,
  completeExperienceSpanAfterPaint,
  measureNextPaint,
} from "../ux/metrics";
import {
  beginPressGesture,
  canCompletePress,
  updatePressGesture,
  type PressGesture,
} from "./pressGesture";

const PRESSABLE_SELECTOR =
  "button:not(:disabled), a[href], [role='button']:not([aria-disabled='true'])";
const FEEDBACK_EXCLUSIONS =
  ".touch-instrument, .performance-menu-button, .rack-details-floating-button, " +
  "[data-rf-press-feedback='none'], [data-rf-press-feedback='local']";
const TOUCH_FEEDBACK_DELAY_MS = 32;

interface ActivePress {
  target: HTMLElement;
  gesture: PressGesture;
  timer: number | null;
  startedAt: number;
}

/**
 * Transitional feedback for legacy RackForge controls.
 *
 * New primitives own their feedback locally. This root keeps existing buttons
 * consistent while screens are migrated and, unlike the former ripple layer,
 * never creates viewport-positioned DOM nodes or lets a drag complete as a tap.
 */
export function InteractionFeedbackRoot() {
  const activeRef = useRef<ActivePress | null>(null);

  useEffect(() => {
    const hideFeedback = (active: ActivePress) => {
      active.target.removeAttribute("data-rf-pressed");
      active.target.classList.remove("rf-interaction-target");
      if (active.timer !== null) window.clearTimeout(active.timer);
      active.timer = null;
    };
    const clear = () => {
      const active = activeRef.current;
      if (!active) return;
      hideFeedback(active);
      activeRef.current = null;
    };
    const reveal = (active: ActivePress) => {
      if (activeRef.current !== active || active.gesture.cancelled) return;
      active.timer = null;
      active.target.classList.add("rf-interaction-target");
      active.target.setAttribute("data-rf-pressed", "true");
      measureNextPaint("input-feedback", active.startedAt);
    };
    const pointerDown = (event: PointerEvent) => {
      if (!event.isPrimary || (event.pointerType === "mouse" && event.button !== 0)) return;
      const origin = event.target instanceof Element ? event.target : null;
      const target = origin?.closest<HTMLElement>(PRESSABLE_SELECTOR);
      if (!target || !target.closest("#root") || target.closest(FEEDBACK_EXCLUSIONS)) return;
      clear();
      const active: ActivePress = {
        target,
        gesture: beginPressGesture(event.pointerId, event.clientX, event.clientY),
        timer: null,
        startedAt: performance.now(),
      };
      activeRef.current = active;
      if (event.pointerType === "mouse") {
        reveal(active);
      } else {
        active.timer = window.setTimeout(() => reveal(active), TOUCH_FEEDBACK_DELAY_MS);
      }
    };
    const pointerMove = (event: PointerEvent) => {
      const active = activeRef.current;
      if (!active) return;
      active.gesture = updatePressGesture(
        active.gesture,
        event.pointerId,
        event.clientX,
        event.clientY,
      );
      if (active.gesture.cancelled) hideFeedback(active);
    };
    const pointerEnd = (event: PointerEvent) => {
      const active = activeRef.current;
      if (!active || active.gesture.pointerId !== event.pointerId) return;
      const releaseTarget = document.elementFromPoint(event.clientX, event.clientY);
      const completed = canCompletePress(active.gesture, event.pointerId) &&
        releaseTarget !== null && active.target.contains(releaseTarget);
      clear();
      if (completed && active.target.dataset.rfHaptic !== "none") hostHaptic("tap");
    };
    const pointerCancel = (event: PointerEvent) => {
      if (activeRef.current?.gesture.pointerId === event.pointerId) clear();
    };
    const click = (event: MouseEvent) => {
      const origin = event.target instanceof Element ? event.target : null;
      const anchor = origin?.closest<HTMLAnchorElement>("a[href]");
      if (!anchor || !anchor.closest("#root")) return;
      const destination = new URL(anchor.href, window.location.href);
      if (destination.origin === window.location.origin) {
        beginExperienceSpan("route-ready", "navigation");
      }
    };

    document.addEventListener("pointerdown", pointerDown, { capture: true, passive: true });
    document.addEventListener("pointermove", pointerMove, { capture: true, passive: true });
    document.addEventListener("pointerup", pointerEnd, { capture: true, passive: true });
    document.addEventListener("pointercancel", pointerCancel, { capture: true, passive: true });
    document.addEventListener("click", click, { capture: true, passive: true });
    document.addEventListener("scroll", clear, { capture: true, passive: true });
    window.addEventListener("blur", clear);
    return () => {
      clear();
      document.removeEventListener("pointerdown", pointerDown, true);
      document.removeEventListener("pointermove", pointerMove, true);
      document.removeEventListener("pointerup", pointerEnd, true);
      document.removeEventListener("pointercancel", pointerCancel, true);
      document.removeEventListener("click", click, true);
      document.removeEventListener("scroll", clear, true);
      window.removeEventListener("blur", clear);
    };
  }, []);

  return null;
}

export function RouteExperienceObserver() {
  const location = useLocation();
  useEffect(() => {
    completeExperienceSpanAfterPaint("route-ready", "navigation");
  }, [location.key]);
  return null;
}
