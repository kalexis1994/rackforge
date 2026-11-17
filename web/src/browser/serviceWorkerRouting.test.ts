import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

const workerSource = readFileSync(
  fileURLToPath(new URL("../../public/sw.js", import.meta.url)),
  "utf8",
);

describe("service worker navigation routing", () => {
  function worker(cachedBody = "old component") {
    const handlers = new Map<string, (event: unknown) => void>();
    const cache = {
      put: vi.fn(async (...args: [unknown, Response]) => { void args; }),
      match: vi.fn(async () => new Response(cachedBody)),
    };
    const fetch = vi.fn(async () => new Response("current component"));
    runInNewContext(workerSource, {
      self: {
        location: new URL("https://example.test/rackforge/sw.js"),
        addEventListener: (name: string, handler: (event: unknown) => void) => handlers.set(name, handler),
      },
      caches: { open: async () => cache }, fetch, URL, Headers, Response,
    });
    async function read(path: string, policy = "default", mode = "cors") {
      let response: Promise<Response> | undefined;
      handlers.get("fetch")?.({
        request: { method: "GET", url: `https://example.test/rackforge/${path}`, mode, cache: policy },
        respondWith: (promise: Promise<Response>) => { response = promise; },
      });
      return (await response)?.text();
    }
    return { read, fetch, cache };
  }

  it.each([
    "demo/storage.json?v=build",
    "demo/rackforge-browser.wasm?v=build",
    "demo/rackforge/plugins/concert-grand/component.wasm?v=build",
    "demo/rackforge/plugins/concert-grand/web/play.js",
    "rackforge-plugin-kit/program-select.js",
  ])("does not return an old deployment for mutable asset %s", async (path) => {
    const { read, fetch, cache } = worker();
    expect(await read(path)).toBe("current component");
    expect(fetch).toHaveBeenCalledWith(expect.anything(), { cache: "no-cache" });
    expect(cache.put).toHaveBeenCalledOnce();
  });

  it("honors a fresh host request even when Cache Storage contains a component", async () => {
    const { read, fetch } = worker();
    expect(await read("demo/rackforge/plugins/concert-grand/component.wasm?v=build", "no-store"))
      .toBe("current component");
    expect(fetch).toHaveBeenCalledWith(expect.anything(), { cache: "no-store" });
  });

  it("keeps cached host files available offline", async () => {
    const { read, fetch } = worker("offline component");
    fetch.mockRejectedValueOnce(new TypeError("offline"));
    expect(await read("demo/rackforge-browser.wasm?v=build", "no-store"))
      .toBe("offline component");
  });

  it("reuses content-hashed assets but bypasses them on a reload", async () => {
    const { read, fetch } = worker("hashed component");
    expect(await read("assets/worklet-a1b2.js")).toBe("hashed component");
    expect(fetch).not.toHaveBeenCalled();
    expect(await read("assets/worklet-a1b2.js", "reload")).toBe("current component");
  });

  it("migrates generated caches without deleting installed packages or another app's cache", async () => {
    const handlers = new Map<string, (event: unknown) => void>();
    const remove = vi.fn(async (name: string) => { void name; return true; });
    const claim = vi.fn(async () => undefined);
    runInNewContext(workerSource, {
      self: {
        location: new URL("https://example.test/rackforge/sw.js"),
        clients: { claim },
        addEventListener: (name: string, handler: (event: unknown) => void) => handlers.set(name, handler),
      },
      caches: {
        keys: async () => ["rackforge-v1", "rackforge-v2", "rackforge-v3", "rackforge-plugin-assets", "another-app"],
        delete: remove,
      },
      URL, Headers, Response,
    });
    let activation: Promise<void> | undefined;
    handlers.get("activate")?.({ waitUntil: (promise: Promise<void>) => { activation = promise; } });
    await activation;
    expect(remove.mock.calls.map(([name]) => name)).toEqual(["rackforge-v1", "rackforge-v2"]);
    expect(claim).toHaveBeenCalledOnce();
  });

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
