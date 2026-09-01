export interface ControllerPresentationTransition {
  openDock?: true;
  navigateTo: string;
}

/**
 * Immersive Touch Controller is deliberately narrower than "landscape".
 *
 * CSS pixels account for display scaling, `any-pointer: coarse` identifies a
 * touch-capable client even when a mouse is also connected, and the aspect
 * ratio/height limits distinguish a phone held sideways from a tablet or a
 * desktop window. Every other viewport uses the controller dock.
 */
export const IMMERSIVE_CONTROLLER_QUERY =
  "(any-pointer: coarse) and (orientation: landscape) and (max-height: 600px) and (max-width: 1200px) and (min-aspect-ratio: 7/4)";

export function isImmersiveControllerViewport({
  width,
  height,
  touchCapable,
}: {
  width: number;
  height: number;
  touchCapable: boolean;
}) {
  return touchCapable
    && width > height
    && height <= 600
    && width <= 1200
    && width / height >= 7 / 4;
}

/**
 * Whether the Touch Controller exists on this host at all.
 *
 * Inside a DAW the host already owns the keyboard: the player has their own
 * controller plugged into it, the plug-in window is small, and an on-screen
 * keyboard there is a dock over the only surface there is room for. So the
 * controller is a desktop and phone thing, not a plug-in one. The rail and the
 * route already left it out of the VST3; the dock did not, because it only
 * ever asked how wide the window was.
 */
export function controllerIsAvailable(vstHost: boolean) {
  return !vstHost;
}

/**
 * Whether it appears as a dock rather than as a surface of its own.
 *
 * Two booleans rather than one object because both are read inside hook
 * dependency lists, and a fresh object every render is memoisation the React
 * compiler cannot see through.
 */
export function controllerIsDockable({
  vstHost,
  immersive,
}: {
  vstHost: boolean;
  immersive: boolean;
}) {
  return controllerIsAvailable(vstHost) && !immersive;
}

export function controllerPresentationTransition({
  dockable,
  dockOpen,
  pathname,
  lastContentRoute,
}: {
  dockable: boolean;
  dockOpen: boolean;
  pathname: string;
  lastContentRoute: string;
}): ControllerPresentationTransition | null {
  if (pathname === "/controller") {
    return dockable
      ? { openDock: true, navigateTo: lastContentRoute }
      : null;
  }
  return !dockable && dockOpen
    ? { navigateTo: "/controller" }
    : null;
}
