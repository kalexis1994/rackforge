import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

describe("active worker recovery", () => {
  it("claims uncontrolled clients when requested, without another activation", async () => {
    const handlers = new Map<string, (event: unknown) => void>();
    const claim = vi.fn(async () => undefined);
    runInNewContext(readFileSync(new URL("../../public/sw.js", import.meta.url), "utf8"), {
      self: {
        location: { hostname: "localhost" },
        clients: { claim },
        addEventListener: (kind: string, handler: (event: unknown) => void) => handlers.set(kind, handler),
      },
    });
    const work: Promise<unknown>[] = [];
    handlers.get("message")!({
      data: { kind: "rackforge-plugin-assets-claim" },
      waitUntil: (promise: Promise<unknown>) => work.push(promise),
    });
    await Promise.all(work);
    expect(claim).toHaveBeenCalledOnce();
    expect(work).toHaveLength(1);
  });
});
