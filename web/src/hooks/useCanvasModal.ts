import { useEffect, useRef, type KeyboardEvent } from "react";

const FOCUSABLE = [
  "button:not([disabled])",
  "select:not([disabled])",
  "input:not([disabled])",
  "textarea:not([disabled])",
  "iframe",
  "[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/**
 * What every modal over the Rack editor's canvas does -- the plugin editor,
 * a MIDI cable's, the audio input's -- so they behave as one: focus goes to
 * its close key as it opens and back where it was when it closes; Escape
 * closes it; Tab stays inside it. The canvas behind is made inert by the
 * editor that opens it.
 */
export function useCanvasModal(onClose: () => void) {
  const sectionRef = useRef<HTMLElement | null>(null);
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const closeLatest = useRef(onClose);
  closeLatest.current = onClose;

  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeRef.current?.focus({ preventScroll: true });
    return () => {
      if (before?.isConnected) before.focus({ preventScroll: true });
    };
  }, []);

  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      closeLatest.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab" || !sectionRef.current) return;
    const stops = [...sectionRef.current.querySelectorAll<HTMLElement>(FOCUSABLE)]
      .filter((element) => element.getClientRects().length > 0);
    if (stops.length === 0) return;
    const first = stops[0];
    const last = stops[stops.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return { sectionRef, closeRef, onKeyDown };
}
