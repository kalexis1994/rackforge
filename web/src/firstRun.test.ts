import { describe, expect, it } from "vitest";

import {
  DEFAULT_INSTRUMENT_ID,
  defaultInstrument,
  firstRunView,
  shouldRunFirstRun,
} from "./firstRun";
import type { PluginWebDescriptor } from "./types";

const instrument = (
  plugin_id: string,
  plugin_name: string,
  kind: PluginWebDescriptor["kind"] = "instrument",
): PluginWebDescriptor => ({
  plugin_id,
  plugin_name,
  version: "1.0.0",
  kind,
  active: false,
  managed: true,
  api_version: 1,
  surfaces: [],
  resources: [],
});

const grand = instrument(DEFAULT_INSTRUMENT_ID, "RF - Concert Grand");
const rf106 = instrument("org.rackforge.rf-106", "RF-106");
const pedal = instrument("org.rackforge.rf-rig", "RF-Rig", "effect");

describe("the instrument a fresh RackForge opens", () => {
  it("is the Concert Grand when this build carries it", () => {
    expect(defaultInstrument([rf106, grand])?.plugin_id).toBe(DEFAULT_INSTRUMENT_ID);
  });

  it("is the first instrument otherwise, so a Minimal build still opens playing", () => {
    expect(defaultInstrument([rf106])?.plugin_id).toBe("org.rackforge.rf-106");
  });

  it("is never an effect: PLAY opens an instrument or nothing", () => {
    expect(defaultInstrument([pedal])).toBeNull();
  });
});

describe("whether the first run takes over", () => {
  it("does not, once this browser has been through it", () => {
    expect(
      shouldRunFirstRun({ completed: true, sessionKnown: true, hasActiveInstance: false }),
    ).toBe(false);
  });

  it("does not on a machine that already has an instrument playing", () => {
    // Whatever the browser remembers, that machine has plainly been used --
    // and the desktop remembers nothing, because it serves on a new port
    // every launch and its interface is a new origin each time.
    expect(
      shouldRunFirstRun({ completed: false, sessionKnown: true, hasActiveInstance: true }),
    ).toBe(false);
  });

  it("waits for the session before judging, so it never flashes over a start", () => {
    expect(
      shouldRunFirstRun({ completed: false, sessionKnown: false, hasActiveInstance: false }),
    ).toBe(false);
  });

  it("does on a fresh machine with nothing active", () => {
    expect(
      shouldRunFirstRun({ completed: false, sessionKnown: true, hasActiveInstance: false }),
    ).toBe(true);
  });
});

describe("what the first run shows", () => {
  it("waits for the engine before it claims to have found anything", () => {
    const view = firstRunView({ catalogStatus: "loading", plugins: [], failure: null });
    expect(view.steps.map((step) => step.state)).toEqual(["working", "waiting", "waiting"]);
    expect(view.progress).toBe(0);
    expect(view.target).toBeNull();
  });

  it("names every instrument it found, and opens the default one", () => {
    const view = firstRunView({
      catalogStatus: "ready",
      plugins: [grand, rf106, pedal],
      failure: null,
    });
    expect(view.steps.map((step) => step.label)).toEqual([
      "Starting the audio engine",
      "RF - Concert Grand",
      "RF-106",
      "Opening RF - Concert Grand",
    ]);
    // The effect is a pedal, not an instrument: it is not offered as a first
    // thing to play, and it is not listed as one.
    expect(view.target?.plugin_id).toBe(DEFAULT_INSTRUMENT_ID);
    expect(view.steps.at(-1)?.state).toBe("working");
    expect(view.progress).toBeCloseTo(3 / 4);
  });

  it("says so when the machine has no instruments at all", () => {
    const view = firstRunView({ catalogStatus: "ready", plugins: [], failure: null });
    expect(view.steps[1]).toMatchObject({ state: "failed", detail: "none installed" });
    expect(view.target).toBeNull();
  });

  it("marks the opening step failed rather than leaving it spinning", () => {
    const view = firstRunView({
      catalogStatus: "ready",
      plugins: [grand],
      failure: "the host refused",
    });
    expect(view.steps.at(-1)?.state).toBe("failed");
  });
});
