import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

const workerSource = readFileSync(
  fileURLToPath(new URL("../../public/sw.js", import.meta.url)),
  "utf8",
);

describe("service worker navigation routing", () => {
  it("caches a packaged plugin iframe under its own URL, not the app shell", async () => {
    const handlers = new Map<string, (event: unknown) => void>();
    const put = vi.fn(async (...args: [unknown, Response]) => { void args; });
    const cache = { put, match: vi.fn(async () => undefined) };
    const fetch = vi.fn(async () => new Response("plugin page"));
    runInNewContext(workerSource, {
      self: {
        location: new URL("https://example.test/rackforge/sw.js"),
        addEventListener: (name: string, handler: (event: unknown) => void) => {
          handlers.set(name, handler);
        },
      },
      caches: { open: async () => cache },
      fetch,
      URL,
      Headers,
      Response,
    });

    const url = "https://example.test/rackforge/demo/rackforge/plugins/concert-grand/web/play.html?v=1";
    let response: Promise<Response> | undefined;
    handlers.get("fetch")?.({
      request: { method: "GET", url, mode: "navigate" },
      respondWith: (promise: Promise<Response>) => { response = promise; },
    });
    expect((await response)?.status).toBe(200);
    expect(fetch).toHaveBeenCalledOnce();
    expect(put).toHaveBeenCalledOnce();
    expect(put.mock.calls[0]?.[0]).toMatchObject({ url });
  });
});
