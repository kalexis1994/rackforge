import { useSyncExternalStore } from "react";
import type { ConnectionStatus } from "./types";

/**
 * Which plugin interfaces are still getting ready.
 *
 * The boot curtain has to wait for the instrument's own interface, and that
 * lives in an iframe the rest of the app cannot see into. So each plugin
 * frame says, here, that it has started and when it has settled -- loaded,
 * or known not to be loadable -- and the curtain waits until nothing it can
 * see is still pending. Unmounting settles a frame too: a surface nobody is
 * showing any more is nothing to wait for.
 */
const pending = new Set<string>();
const listeners = new Set<() => void>();
let count = 0;

function publish() {
  count = pending.size;
  for (const listener of listeners) listener();
}

export function surfaceStarted(key: string) {
  if (pending.has(key)) return;
  pending.add(key);
  publish();
}

export function surfaceSettled(key: string) {
  if (!pending.delete(key)) return;
  publish();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** How many plugin interfaces are still getting ready. */
export function pendingSurfaceCount(): number {
  return count;
}

export function usePendingSurfaceCount(): number {
  return useSyncExternalStore(subscribe, pendingSurfaceCount, pendingSurfaceCount);
}

/** For tests: forget every surface. */
export function resetSurfaces() {
  pending.clear();
  publish();
}

/** Shown at least this long, so a warm start does not flash the curtain. */
export const BOOT_CURTAIN_MINIMUM_MS = 700;
/** Everything has to have been ready this long before the curtain lifts: the
    moment the session arrives, the page has not yet mounted the instrument
    it is about to wait for. */
export const BOOT_CURTAIN_SETTLE_MS = 250;
/** Lifted after this long whatever is still pending. The page under it shows
    its own state from there on; nobody is trapped behind a loader. */
export const BOOT_CURTAIN_MAXIMUM_MS = 20_000;
/** The fade: twice `--rf-motion-emphasized`, as the CSS transition is. */
export const BOOT_CURTAIN_FADE_MS = 640;

export interface BootInputs {
  connection: ConnectionStatus;
  sessionKnown: boolean;
  catalogStatus: "idle" | "loading" | "ready" | "error";
  pendingSurfaces: number;
  /** The first-run screen has the stage; the curtain gives it up. */
  firstRunActive: boolean;
  /** The name of the instrument being opened, for the detail line. */
  activeInstrument?: string;
}

export interface BootPhase {
  ready: boolean;
  detail: string;
}

/**
 * Whether RackForge is ready to be used, and what it is doing if not.
 *
 * Ready means everything the first screen needs has arrived: the session,
 * the plugin catalogue, and the interface of every plugin on the page. A
 * catalogue that failed to load is ready too -- the page says so itself, and
 * the curtain must not hide it.
 */
export function bootPhase(inputs: BootInputs): BootPhase {
  if (inputs.firstRunActive) return { ready: true, detail: "" };
  if (!inputs.sessionKnown) {
    return {
      ready: false,
      detail: inputs.connection === "offline"
        ? "Waiting for the RackForge engine…"
        : "Starting the engine…",
    };
  }
  if (inputs.catalogStatus === "idle" || inputs.catalogStatus === "loading") {
    return { ready: false, detail: "Loading instruments…" };
  }
  if (inputs.pendingSurfaces > 0) {
    return {
      ready: false,
      detail: inputs.activeInstrument ? `Opening ${inputs.activeInstrument}…` : "Opening the instrument…",
    };
  }
  return { ready: true, detail: "" };
}
