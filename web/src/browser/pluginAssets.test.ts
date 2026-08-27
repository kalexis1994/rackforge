import { afterEach, describe, expect, it, vi } from "vitest";
import {
  canServePluginAssets,
  declaredPluginAssetsPublished,
  isPackagedPluginRoot,
  pluginAssetUrl,
  publishPluginAssets,
  versionPluginAssetUrl,
} from "./pluginAssets";
import {
  linkedPackageMutationEvents,
  waitForLinkedStoragePublication,
} from "./protocol";

class MemoryCache {
  readonly entries = new Map<string, Response>();

  private url(input: RequestInfo | URL): string {
    const value = input instanceof Request ? input.url : String(input);
    return new URL(value, "https://rackforge.test/").href;
  }

  async put(input: RequestInfo | URL, response: Response) {
    this.entries.set(this.url(input), response.clone());
  }

  async keys() {
    return [...this.entries.keys()].map((url) => new Request(url));
  }

  async delete(input: RequestInfo | URL) {
    return this.entries.delete(this.url(input));
  }

  async match(path: string) {
    const target = new URL(path, "https://rackforge.test/");
    for (const [url, response] of this.entries) {
      const candidate = new URL(url);
      if (candidate.pathname === target.pathname) return response.clone();
    }
    return undefined;
  }
}

function seed(path: string, body: string) {
  return { path, bytes: new TextEncoder().encode(body) };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("browser plugin asset routing", () => {
  it("serves bundled plugin surfaces directly without waiting for a worker", () => {
    expect(isPackagedPluginRoot("plugins/concert-grand")).toBe(true);
    expect(canServePluginAssets("plugins/concert-grand", false)).toBe(true);
    expect(pluginAssetUrl("plugins/concert-grand", "web/play.html", "1.2.3"))
      .toBe("/demo/rackforge/plugins/concert-grand/web/play.html?v=1.2.3");
  });

  it("keeps installed plugin surfaces behind the worker-backed route", () => {
    const root = "store/packages/org.rackforge.test/1.0.0";
    expect(isPackagedPluginRoot(root)).toBe(false);
    expect(canServePluginAssets(root, false)).toBe(false);
    expect(canServePluginAssets(root, true)).toBe(true);
    expect(pluginAssetUrl(root, "web/play.html", "1.0.0"))
      .toBe("/plugin-assets/store/packages/org.rackforge.test/1.0.0/web/play.html?v=1.0.0");
  });

  it("does not advertise a dynamic surface before every declared asset is published", () => {
    const root = "store/packages/org.rackforge.test/1.0.0";
    const entries = ["web/play.html", "branding/icon.png"];
    expect(declaredPluginAssetsPublished(root, entries, new Set([
      `${root}/web/play.html`,
    ]))).toBe(false);
    expect(declaredPluginAssetsPublished(root, entries, new Set([
      `${root}/web/play.html`,
      `${root}/branding/icon.png`,
    ]))).toBe(true);
    expect(declaredPluginAssetsPublished(
      "plugins/concert-grand",
      entries,
      new Set(),
    )).toBe(true);
  });

  it("normalizes Windows package roots before choosing the route", () => {
    expect(isPackagedPluginRoot("plugins\\concert-grand")).toBe(true);
  });

  it("changes installed iframe identity after a completed publication only", () => {
    const root = "store/packages/org.rackforge.test/1.0.0";
    const url = pluginAssetUrl(root, "web/play.html", "1.0.0");
    expect(versionPluginAssetUrl(url, root, 4)).toBe(`${url}&assets=4`);
    expect(versionPluginAssetUrl(
      pluginAssetUrl("plugins/concert-grand", "web/play.html", "1.0.0"),
      "plugins/concert-grand",
      4,
    )).toBe("/demo/rackforge/plugins/concert-grand/web/play.html?v=1.0.0");
  });

  it("publishes a complete dynamic UI and every branding asset before resolving", async () => {
    const cache = new MemoryCache();
    vi.stubGlobal("caches", {
      open: vi.fn(async () => cache),
    });
    const root = "store/packages/org.rackforge.test/1.0.0";
    const files = [
      seed(`${root}/web/play.html`, "<main>Test</main>"),
      seed(`${root}/web/app.js`, "boot()"),
      seed(`${root}/web/app_bg.wasm`, "wasm"),
      seed(`${root}/web/app.css`, "main{}"),
      seed(`${root}/branding/icon.png`, "icon"),
      seed(`${root}/branding/banner.png`, "banner"),
      seed(`${root}/branding/splash.png`, "splash"),
    ];

    const published = await publishPluginAssets(files);

    for (const file of files) {
      expect(published.has(file.path)).toBe(true);
      const response = await cache.match(`/plugin-assets/${file.path}`);
      expect(response?.ok).toBe(true);
      expect(await response?.text()).toBe(new TextDecoder().decode(file.bytes));
    }
    expect((await cache.match(`/plugin-assets/${root}/web/app_bg.wasm`))?.headers
      .get("content-type")).toBe("application/wasm");
  });

  it("keeps the publication promise pending until the last asset write completes", async () => {
    const cache = new MemoryCache();
    vi.stubGlobal("caches", { open: vi.fn(async () => cache) });
    let releaseWasm: (() => void) | undefined;
    const wasmWrite = new Promise<void>((resolve) => {
      releaseWasm = resolve;
    });
    const originalPut = cache.put.bind(cache);
    vi.spyOn(cache, "put").mockImplementation(async (input, response) => {
      if (String(input).includes("app_bg.wasm")) await wasmWrite;
      await originalPut(input, response);
    });
    const root = "store/packages/org.rackforge.test/1.0.0";
    const publication = publishPluginAssets([
      seed(`${root}/web/play.html`, "play"),
      seed(`${root}/web/app.js`, "js"),
      seed(`${root}/web/app_bg.wasm`, "wasm"),
    ]);
    let settled = false;
    void publication.then(() => {
      settled = true;
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);
    releaseWasm?.();
    await publication;
    expect(settled).toBe(true);
  });

  it("coordinates the real install event order with immediate surface discovery", async () => {
    const cache = new MemoryCache();
    vi.stubGlobal("caches", { open: vi.fn(async () => cache) });
    let releaseLastAsset: (() => void) | undefined;
    const lastAsset = new Promise<void>((resolve) => {
      releaseLastAsset = resolve;
    });
    const originalPut = cache.put.bind(cache);
    vi.spyOn(cache, "put").mockImplementation(async (input, response) => {
      if (String(input).includes("splash.png")) await lastAsset;
      await originalPut(input, response);
    });
    const root = "store/packages/org.rackforge.test/1.0.0";
    const files = [
      seed(`${root}/web/play.html`, "play"),
      seed(`${root}/web/app.js`, "js"),
      seed(`${root}/web/app_bg.wasm`, "wasm"),
      seed(`${root}/branding/icon.png`, "icon"),
      seed(`${root}/branding/banner.png`, "banner"),
      seed(`${root}/branding/splash.png`, "splash"),
    ];
    const [storage, response] = linkedPackageMutationEvents(
      51,
      JSON.stringify({ ok: true, installed: { plugin_id: "org.rackforge.test" } }),
      files,
      true,
    );
    let published: ReadonlySet<string> = new Set();
    const publication = publishPluginAssets(storage.files).then((paths) => {
      published = paths;
    });
    const completion = waitForLinkedStoragePublication(
      response,
      new Map([[storage.operation_id!, publication]]),
    );
    let installReady = false;
    void completion.then(() => {
      installReady = true;
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(installReady).toBe(false);
    expect(declaredPluginAssetsPublished(
      root,
      ["web/play.html", "branding/splash.png"],
      published,
    )).toBe(false);

    releaseLastAsset?.();
    await completion;
    expect(installReady).toBe(true);
    expect(declaredPluginAssetsPublished(
      root,
      ["web/play.html", "branding/splash.png"],
      published,
    )).toBe(true);
    for (const file of files) {
      expect(await cache.match(`/plugin-assets/${file.path}`)).toBeDefined();
    }
  });

  it("removes stale package files and supports reinstalling in the same session", async () => {
    const cache = new MemoryCache();
    vi.stubGlobal("caches", {
      open: vi.fn(async () => cache),
    });
    const root = "store/packages/org.rackforge.test/1.0.0";
    const path = `${root}/web/play.html`;

    await publishPluginAssets([seed(path, "first install")]);
    expect(await (await cache.match(`/plugin-assets/${path}`))?.text()).toBe("first install");

    await publishPluginAssets([]);
    expect(await cache.match(`/plugin-assets/${path}`)).toBeUndefined();
    expect(cache.entries.size).toBe(0);

    await publishPluginAssets([seed(path, "reinstalled")]);
    expect(await (await cache.match(`/plugin-assets/${path}`))?.text()).toBe("reinstalled");
    expect(cache.entries.size).toBe(1);
  });
});
