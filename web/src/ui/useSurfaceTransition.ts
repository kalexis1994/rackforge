import { useLayoutEffect, useRef, type RefObject } from "react";

export const SURFACE_TRANSITION_DURATION_MS = 220;

export function shouldAnimateSurfaceTransition({
  disabled,
  firstRender,
  reducedMotion,
  documentVisible,
}: {
  disabled: boolean;
  firstRender: boolean;
  reducedMotion: boolean;
  documentVisible: boolean;
}) {
  return !disabled && !firstRender && !reducedMotion && documentVisible;
}

/**
 * Adds continuity when a route changes without remounting or wrapping the
 * route tree. The CSS animation only changes opacity: transforms would create
 * a containing block and temporarily displace fixed dialogs, graph overlays
 * and plugin surfaces. A class-based animation also works in older WebViews
 * that do not expose the Web Animations API.
 */
export function useSurfaceTransition(
  surfaceRef: RefObject<HTMLElement | null>,
  transitionKey: string,
  disabled = false,
) {
  const previousKeyRef = useRef(transitionKey);

  useLayoutEffect(() => {
    const firstRender = previousKeyRef.current === transitionKey;
    previousKeyRef.current = transitionKey;
    const element = surfaceRef.current;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (
      !element ||
      !shouldAnimateSurfaceTransition({
        disabled,
        firstRender,
        reducedMotion,
        documentVisible: document.visibilityState !== "hidden",
      })
    ) {
      return;
    }

    const finish = () => element.classList.remove("rf-route-enter");
    element.classList.remove("rf-route-enter");
    // Restart the transition even when two navigations settle in the same
    // frame. This layout read never touches the audio or plugin data paths.
    void element.offsetWidth;
    element.classList.add("rf-route-enter");
    element.addEventListener("animationend", finish, { once: true });
    const timeout = window.setTimeout(finish, SURFACE_TRANSITION_DURATION_MS * 2);
    return () => {
      window.clearTimeout(timeout);
      element.removeEventListener("animationend", finish);
      finish();
    };
  }, [disabled, surfaceRef, transitionKey]);
}
