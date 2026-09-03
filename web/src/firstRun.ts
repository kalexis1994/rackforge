/**
 * The first time RackForge opens on a machine.
 *
 * A fresh installation has instruments but no session: the host installs
 * every bundled package before it serves anything, and then nothing makes
 * one of them the instrument you are playing. PLAY opened on "No instrument
 * active", which is true and useless — the player has done nothing wrong and
 * has nothing to fix. Worse, it stayed that way after the packages arrived,
 * because nothing was watching for them.
 *
 * So the first run is a state of its own: the interface waits for the engine
 * and the catalogue, names each instrument it finds, opens the default one,
 * and remembers that it did. What it must never do is trap anyone — every
 * path out of here, including failure and a machine with no instruments at
 * all, ends with the interface it was covering.
 */

import type { PluginWebDescriptor } from "./types";

/** The instrument a fresh RackForge opens with. */
export const DEFAULT_INSTRUMENT_ID = "org.rackforge.concert-grand";

const COMPLETED_KEY = "rackforge.firstRun.completed";

export type FirstRunStage = "engine" | "instruments" | "opening" | "done";

export type FirstRunStepState = "waiting" | "working" | "done" | "failed";

export interface FirstRunStep {
  id: string;
  label: string;
  detail?: string;
  state: FirstRunStepState;
}

/**
 * Why a first run stopped, and therefore what to offer.
 *
 * The kind matters more than the words: a machine with no instruments needs
 * the Plugin Manager, and everything else needs another attempt. Offering
 * "continue" for either would hand the player exactly the dead end this
 * screen exists to prevent -- an interface with nothing active in it.
 */
export type FirstRunFailureKind =
  | "no_instruments"
  | "catalogue"
  | "activation"
  | "timeout";

export interface FirstRunFailure {
  kind: FirstRunFailureKind;
  message: string;
}

export interface FirstRunInputs {
  /** The plugin catalogue's own status. */
  catalogStatus: "idle" | "loading" | "ready" | "error";
  plugins: PluginWebDescriptor[];
  failure: FirstRunFailure | null;
}

export interface FirstRunView {
  steps: FirstRunStep[];
  /** 0..1, for the bar. */
  progress: number;
  /** The instrument a fresh machine should open, or null if it has none. */
  target: PluginWebDescriptor | null;
}

/**
 * Which instrument a fresh RackForge opens: the Concert Grand if this build
 * carries it, and otherwise the first instrument in the catalogue, so a
 * Minimal build with one instrument added by hand still opens playing.
 */
export function defaultInstrument(
  plugins: PluginWebDescriptor[],
): PluginWebDescriptor | null {
  const instruments = plugins.filter((plugin) => plugin.kind === "instrument");
  return (
    instruments.find((plugin) => plugin.plugin_id === DEFAULT_INSTRUMENT_ID) ??
    instruments[0] ??
    null
  );
}

/** The screen's own reading of where the first run has got to. */
export function firstRunView(inputs: FirstRunInputs): FirstRunView {
  const { catalogStatus, plugins, failure } = inputs;
  const engineReady = catalogStatus === "ready" || catalogStatus === "error";
  const instruments = plugins.filter((plugin) => plugin.kind === "instrument");
  const target = defaultInstrument(plugins);
  const steps: FirstRunStep[] = [
    {
      id: "engine",
      label: "Starting the audio engine",
      state: failure && !engineReady ? "failed" : engineReady ? "done" : "working",
    },
  ];
  if (!engineReady) {
    steps.push({
      id: "instruments",
      label: "Preparing your instruments",
      state: "waiting",
    });
  } else if (instruments.length === 0) {
    steps.push({
      id: "instruments",
      label: "Preparing your instruments",
      detail: "none installed",
      state: "failed",
    });
  } else {
    for (const instrument of instruments) {
      steps.push({
        id: instrument.plugin_id,
        label: instrument.plugin_name,
        detail: instrument.version ? `v${instrument.version}` : undefined,
        state: "done",
      });
    }
  }
  // The last step is under way exactly when there is something to open and
  // nothing has gone wrong: the activation starts on the same reading.
  steps.push({
    id: "opening",
    label: target ? `Opening ${target.plugin_name}` : "Opening your instrument",
    state: failure
      ? "failed"
      : engineReady && instruments.length > 0
        ? "working"
        : "waiting",
  });
  const done = steps.filter((step) => step.state === "done").length;
  return { steps, progress: done / steps.length, target };
}

/** Whether this browser has been through a first run already. */
export function readFirstRunCompleted(): boolean {
  try {
    return window.localStorage.getItem(COMPLETED_KEY) === "1";
  } catch {
    // A browser that refuses storage does the first run every time, which is
    // a screen that opens an instrument -- not a failure.
    return false;
  }
}

export function markFirstRunCompleted(): void {
  try {
    window.localStorage.setItem(COMPLETED_KEY, "1");
  } catch {
    // Nothing to do: the run still finishes, it is just not remembered.
  }
}

/**
 * Whether the first run should take over the interface.
 *
 * Three readings, and the session is the authority. A machine that already
 * has an instrument playing has plainly been used before, whatever this
 * browser remembers -- which matters more than it looks: the desktop serves
 * on a fresh port every launch, so its interface is a new origin each time
 * and remembers nothing. And until the session has arrived there is nothing
 * to judge: taking over then would flash this screen over every start.
 */
export function shouldRunFirstRun(inputs: {
  completed: boolean;
  sessionKnown: boolean;
  hasActiveInstance: boolean;
}): boolean {
  if (!inputs.sessionKnown) return false;
  return !inputs.completed && !inputs.hasActiveInstance;
}
