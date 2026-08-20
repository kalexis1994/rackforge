import { describe, expect, it } from "vitest";
import { engineFailureEvent } from "./protocol";

describe("browser engine failures", () => {
  it("answers the package request that failed", () => {
    const event = engineFailureEvent(
      {
        kind: "package",
        id: 41,
        action: "inspect",
        payload: new Uint8Array(),
      },
      "memory limit reached",
    );

    expect(event).toEqual({
      kind: "response",
      id: 41,
      response: JSON.stringify({ ok: false, error: "memory limit reached" }),
    });
  });

  it("keeps control errors on the control response contract", () => {
    const event = engineFailureEvent(
      { kind: "request", id: 7, request: "{}" },
      "bad request",
    );

    expect(event).toEqual({
      kind: "response",
      id: 7,
      response: JSON.stringify({
        status: "error",
        code: "internal",
        message: "bad request",
      }),
    });
  });

  it("reports boot failures only for boot commands", () => {
    expect(
      engineFailureEvent(
        {
          kind: "boot",
          wasm: new Uint8Array(),
          files: [],
          maximumFrames: 128,
          channels: 2,
        },
        "could not boot",
      ),
    ).toEqual({
      kind: "booted",
      ok: false,
      error: "could not boot",
      warnings: [],
    });
    expect(
      engineFailureEvent({ kind: "midi", data: [0x90, 60, 100], length: 3 }, "late"),
    ).toBeNull();
  });
});
