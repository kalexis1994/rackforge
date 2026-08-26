export interface ControllerPresentationTransition {
  openDock?: true;
  navigateTo: string;
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
