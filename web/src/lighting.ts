/**
 * Lighting condition — RackForge's theme preference.
 *
 * The faceplate has two lighting conditions, DAYLIGHT and STAGE, and the
 * player picks one or lets the operating system decide. The choice is a
 * `data-theme` stamp on the document element, which is what the token
 * layer in styles.css keys off:
 *
 *   (no stamp)          → follow prefers-color-scheme
 *   data-theme="light"  → DAYLIGHT, even on a dark desktop
 *   data-theme="dark"   → STAGE, even on a light desktop
 *
 * Applied before the first render so the panel never flashes the wrong
 * condition on the way in.
 */

export type LightingMode = "auto" | "light" | "dark";

const STORAGE_KEY = "rackforge-lighting";
const MODES: readonly LightingMode[] = ["auto", "light", "dark"];

export function readLighting(): LightingMode {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved && MODES.includes(saved as LightingMode)) {
      return saved as LightingMode;
    }
  } catch {
    // A private window or a host with storage disabled: follow the system.
  }
  return "auto";
}

export function applyLighting(mode: LightingMode): void {
  const root = document.documentElement;
  if (mode === "auto") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", mode);
  }
}

export function storeLighting(mode: LightingMode): void {
  applyLighting(mode);
  try {
    window.localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // The choice still applies to this session; it just will not persist.
  }
}

/** The condition actually being rendered, with "auto" resolved. */
export function resolveLighting(mode: LightingMode): "light" | "dark" {
  if (mode !== "auto") {
    return mode;
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}
