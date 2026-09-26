import { afterEach, describe, expect, it } from "vitest";
import {
  bootPhase,
  pendingSurfaceCount,
  resetSurfaces,
  surfaceSettled,
  surfaceStarted,
  type BootInputs,
} from "./bootReadiness";

const ready: BootInputs = {
  connection: "online",
  sessionKnown: true,
  catalogStatus: "ready",
  pendingSurfaces: 0,
  firstRunActive: false,
  activeInstrument: "RF-Tines",
};

describe("bootPhase", () => {
  it("is ready only once the session, the catalogue and every interface are", () => {
    expect(bootPhase(ready).ready).toBe(true);
    expect(bootPhase({ ...ready, sessionKnown: false })).toEqual({
      ready: false,
      detail: "Starting the engine…",
    });
    expect(bootPhase({ ...ready, catalogStatus: "loading" })).toEqual({
      ready: false,
      detail: "Loading instruments…",
    });
    expect(bootPhase({ ...ready, pendingSurfaces: 1 })).toEqual({
      ready: false,
      detail: "Opening RF-Tines…",
    });
  });

  it("does not hide a catalogue that failed: the page says so itself", () => {
    expect(bootPhase({ ...ready, catalogStatus: "error" }).ready).toBe(true);
  });

  it("gives the stage to the first-run screen, whatever else is pending", () => {
    expect(
      bootPhase({ ...ready, sessionKnown: false, pendingSurfaces: 2, firstRunActive: true }).ready,
    ).toBe(true);
  });

  it("says what it is waiting for while the engine is unreachable", () => {
    expect(bootPhase({ ...ready, sessionKnown: false, connection: "offline" }).detail).toBe(
      "Waiting for the RackForge engine…",
    );
  });
});

describe("the surface registry", () => {
  afterEach(() => resetSurfaces());

  it("counts a surface once, and settling one that never started changes nothing", () => {
    surfaceStarted("a:play");
    surfaceStarted("a:play");
    surfaceStarted("b:play");
    expect(pendingSurfaceCount()).toBe(2);
    surfaceSettled("never:play");
    expect(pendingSurfaceCount()).toBe(2);
    surfaceSettled("a:play");
    surfaceSettled("a:play");
    expect(pendingSurfaceCount()).toBe(1);
    surfaceSettled("b:play");
    expect(pendingSurfaceCount()).toBe(0);
  });
});
