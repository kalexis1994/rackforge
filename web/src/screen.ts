/**
 * Screen glass — whether the plugin surface is shown behind a cover.
 *
 * A plugin does not render *in* RackForge, it renders on a screen that
 * RackForge is the machine for. With the glass on, the surface is seen through
 * an acrylic cover: the corners fall off, a broad sheen crosses it, and there
 * is the film of dust and handling any panel picks up.
 *
 * It is a stamp on the document element rather than a class on the frame,
 * because the plugin surface is mounted in several places — PLAY, a rack slot
 * popover — and they all sit behind the same cover:
 *
 *   (no stamp)           → glass off
 *   data-screen="glass"  → glass on
 *
 * Off is the default, and the default carries no stamp: reading a dense plugin
 * panel through a cover is harder, so the panel is bare until a player asks
 * for the glass. That way the surface also never flashes covered before the
 * stored choice is read.
 */

export type ScreenGlass = "glass" | "clean";

const STORAGE_KEY = "rackforge-screen";
const MODES: readonly ScreenGlass[] = ["glass", "clean"];

export function readScreenGlass(): ScreenGlass {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved && MODES.includes(saved as ScreenGlass)) {
      return saved as ScreenGlass;
    }
  } catch {
    // A private window or a host with storage disabled: no cover.
  }
  return "clean";
}

export function applyScreenGlass(mode: ScreenGlass): void {
  const root = document.documentElement;
  if (mode === "glass") {
    root.setAttribute("data-screen", mode);
  } else {
    root.removeAttribute("data-screen");
  }
}

export function storeScreenGlass(mode: ScreenGlass): void {
  applyScreenGlass(mode);
  try {
    window.localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // The choice still applies to this session; it just will not persist.
  }
}
