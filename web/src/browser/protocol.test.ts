import { describe, expect, it } from "vitest";
import {
  engineFailureEvent,
  linkedPackageMutationEvents,
  waitForLinkedStoragePublication,
} from "./protocol";

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

  it("does not expose a package response until its exact storage publication finishes", async () => {
    let releasePublication: (() => void) | undefined;
    const publication = new Promise<void>((resolve) => {
      releasePublication = resolve;
    });
    const event = {
      kind: "response" as const,
      id: 19,
      response: JSON.stringify({ ok: true }),
      storage_operation_id: 19,
    };
    const waiting = waitForLinkedStoragePublication(event, new Map([[19, publication]]));
    let settled = false;
    void waiting.then(() => {
      settled = true;
    });

    await Promise.resolve();
    expect(settled).toBe(false);
    releasePublication?.();
    await waiting;
    expect(settled).toBe(true);
  });

  it("orders a successful package snapshot before its linked response", () => {
    const events = linkedPackageMutationEvents(
      31,
      JSON.stringify({ ok: true }),
      [{ path: "store/packages/test/1/web/play.html", bytes: new Uint8Array([1]) }],
      true,
    );
    expect(events.map((event) => event.kind)).toEqual(["storage", "response"]);
    expect(events[0]).toMatchObject({
      operation_id: 31,
      publish_plugin_assets: true,
    });
    expect(events[1]).toMatchObject({
      id: 31,
      storage_operation_id: 31,
    });
  });

  it("rejects a linked response when its storage snapshot never arrived", async () => {
    await expect(
      waitForLinkedStoragePublication(
        {
          kind: "response",
          id: 23,
          response: JSON.stringify({ ok: true }),
          storage_operation_id: 23,
        },
        new Map(),
      ),
    ).rejects.toThrow("before storage operation 23");
  });
});
