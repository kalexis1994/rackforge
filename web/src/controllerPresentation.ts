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
