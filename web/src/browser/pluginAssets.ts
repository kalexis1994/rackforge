/**
 * Publishes the files a plugin's own interface is made of.
 *
 * A plugin may ship its own PLAY and CONFIG pages, and RackForge renders them
 * in an iframe. A networked host serves those files over HTTP from the
 * installed package. A page has no HTTP server — but it does have a service
 * worker, which is the same thing seen from the other side: RackForge writes
 * the package's files into a cache under `plugin-assets/`, and the worker
 * answers from it.
 *
 * The files come from the host's own storage, so a plugin's interface is
 * exactly the one inside its package, and an uninstalled plugin's files
 * disappear with it.
 */

import { assetUrl } from "../assets";
import type { SeedFile } from "./protocol";
import { ensureServiceWorker } from "./pwa";

const CACHE = "rackforge-plugin-assets";
const PLUGIN_ASSET_PROTOCOL = 1;
/** Path the service worker recognises, below the site's base. */
export const PLUGIN_ASSET_PREFIX = "plugin-assets/";
/** Where packages shipped with the site sit on disk. */
const PACKAGED_SOURCE_PREFIX = "demo/rackforge/";

/** Guessed from the extension: a cache entry has no server to ask. */
function contentType(path: string): string {
  const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  switch (extension) {
    case "html":
      return "text/html; charset=utf-8";
    case "js":
    case "mjs":
      return "text/javascript; charset=utf-8";
    case "css":
      return "text/css; charset=utf-8";
    case "json":
      return "application/json; charset=utf-8";
    case "svg":
      return "image/svg+xml";
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "webp":
      return "image/webp";
    case "woff2":
      return "font/woff2";
    case "wasm":
      return "application/wasm";
    default:
      return "application/octet-stream";
  }
}

/**
 * Waits until the service worker is answering requests for this page.
 *
 * A plugin's interface is loaded by an iframe, which goes to the network like
 * any other request: if the worker is not controlling the page yet, that
 * request escapes to the site and 404s. RackForge therefore does not hand out
 * a plugin's URL until there is something to serve it.
 *
 * Resolves to `false` where no worker can run — a private window, or a browser
 * without them — so plugin interfaces are reported as unavailable rather than
 * advertised and then broken.
 */
interface PluginAssetController {
  postMessage(message: unknown, transfer: Transferable[]): void;
}

/** Proves that the controller understands RackForge's virtual plugin route. */
export function supportsPluginAssetProtocol(
  controller: PluginAssetController,
  timeoutMs = 750,
): Promise<boolean> {
  if (typeof MessageChannel === "undefined") return Promise.resolve(false);
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    let settled = false;
    const settle = (supported: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      channel.port1.close();
      resolve(supported);
    };
    const timer = setTimeout(() => settle(false), timeoutMs);
    channel.port1.onmessage = (event: MessageEvent<{
      kind?: string;
      protocol?: number;
    }>) => settle(
      event.data?.kind === "rackforge-plugin-assets-capabilities"
        && event.data.protocol === PLUGIN_ASSET_PROTOCOL,
    );
    try {
      controller.postMessage(
        { kind: "rackforge-plugin-assets-capabilities" },
        [channel.port2],
      );
    } catch {
      settle(false);
    }
  });
}

let serving: Promise<boolean> | null = null;

async function establishServing(timeoutMs: number): Promise<boolean> {
  if (!("serviceWorker" in navigator)) return false;
  const registration = await ensureServiceWorker();
  if (!registration) return false;

  // Explicitly check for an update. `controller !== null` only means *some*
  // worker owns the page; it says nothing about support for plugin-assets.
  // The current worker calls skipWaiting + clients.claim, so controllerchange
  // can complete this without asking the performer for a reload.
  await registration.update().catch(() => undefined);

  const deadline = Date.now() + timeoutMs;
  let checked: ServiceWorker | null = null;
  while (Date.now() < deadline) {
    const controller = navigator.serviceWorker.controller;
    if (controller && controller !== checked) {
      checked = controller;
      const remaining = Math.max(1, deadline - Date.now());
      if (await supportsPluginAssetProtocol(controller, Math.min(750, remaining))) {
        return true;
      }
    }
    const remaining = deadline - Date.now();
    if (remaining <= 0) break;
    await new Promise<void>((resolve) => {
      const settle = () => {
        navigator.serviceWorker.removeEventListener("controllerchange", settle);
        window.clearTimeout(timer);
        resolve();
      };
      const timer = window.setTimeout(settle, Math.min(100, remaining));
      navigator.serviceWorker.addEventListener("controllerchange", settle, { once: true });
    });
  }
  return false;
}

export function whenServing(timeoutMs = 5_000): Promise<boolean> {
  if (serving) return serving;
  serving = establishServing(timeoutMs).then((available) => {
    if (!available) serving = null;
    return available;
  }, () => {
    serving = null;
    return false;
  });
  return serving;
}

/** Packages shipped with the web build have ordinary static URLs. */
export function isPackagedPluginRoot(packageRoot: string): boolean {
  const root = packageRoot.replace(/\\/g, "/").replace(/^\/+/, "");
  return root === "plugins" || root.startsWith("plugins/");
}

/** Whether a descriptor can safely advertise its branding and web surfaces. */
export function canServePluginAssets(packageRoot: string, workerServing: boolean): boolean {
  return isPackagedPluginRoot(packageRoot) || workerServing;
}

/** The URL a file inside a package is published at. */
export function pluginAssetUrl(packageRoot: string, entry: string, version: string): string {
  const path = `${packageRoot}/${entry}`.replace(/\\/g, "/").replace(/^\/+/, "");
  // Packages shipped by the site always live on disk, including production.
  // An installed one exists solely in host storage and needs the worker.
  const prefix =
    isPackagedPluginRoot(packageRoot)
      ? PACKAGED_SOURCE_PREFIX
      : PLUGIN_ASSET_PREFIX;
  return `${assetUrl(prefix)}${path}?v=${encodeURIComponent(version)}`;
}

/** Canonical package-relative key used by publication readiness checks. */
export function pluginAssetPath(packageRoot: string, entry: string): string {
  return `${packageRoot}/${entry}`
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/{2,}/g, "/");
}

/**
 * Adds a publication generation to installed-plugin URLs.
 *
 * A same-version reinstall and a deterministic recovery must produce a new
 * iframe identity even though the plugin's semantic version did not change.
 * Bundled packages stay on their immutable static URL.
 */
export function versionPluginAssetUrl(
  url: string,
  packageRoot: string,
  publicationGeneration: number,
): string {
  if (isPackagedPluginRoot(packageRoot) || publicationGeneration <= 0) return url;
  return `${url}&assets=${publicationGeneration}`;
}

/** True only after every catalog-declared dynamic file crossed the cache barrier. */
export function declaredPluginAssetsPublished(
  packageRoot: string,
  entries: readonly string[],
  publishedPaths: ReadonlySet<string>,
): boolean {
  return isPackagedPluginRoot(packageRoot)
    || entries.every((entry) => publishedPaths.has(pluginAssetPath(packageRoot, entry)));
}

/**
 * Republishes every packaged file the host currently holds.
 *
 * Called with the same storage snapshot the page files in IndexedDB, so the
 * published files and the installed packages can never disagree. Entries no
 * longer present are dropped, which is what makes an uninstall take effect.
 */
export async function publishPluginAssets(files: SeedFile[]): Promise<ReadonlySet<string>> {
  if (!("caches" in globalThis)) return new Set();
  const cache = await caches.open(CACHE);
  const published = new Set<string>();
  const writes: Array<{ file: SeedFile; url: string }> = [];

  for (const file of files) {
    // Only package contents are published; the host's private storage —
    // presets, plugin state, the performance library — stays private.
    if (!file.path.startsWith("plugins/") && !file.path.startsWith("store/packages/")) {
      continue;
    }
    const url = `${assetUrl(PLUGIN_ASSET_PREFIX)}${file.path}`;
    published.add(pluginAssetPath("", file.path));
    writes.push({ file, url });
  }

  // Publication is a barrier: callers cannot advertise a package until all
  // of its HTML, JavaScript, CSS, Wasm and branding writes have completed.
  // Keep concurrency bounded: sampled instruments may contain hundreds of
  // megabytes and cloning every Response at once would spike mobile memory.
  let nextWrite = 0;
  const write = async () => {
    while (nextWrite < writes.length) {
      const { file, url } = writes[nextWrite++];
      await cache.put(
        url,
        new Response(file.bytes.slice(), {
          headers: {
            "content-type": contentType(file.path),
            "cache-control": "no-cache",
          },
        }),
      );
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(4, writes.length) }, () => write()),
  );

  // Cache.put resolving is the publication barrier. Verify the resulting
  // entries instead of trusting only our in-memory path set, so a quota or
  // browser cache failure can never produce a catalog that advertises 404s.
  for (const { url } of writes) {
    if (!(await cache.match(url, { ignoreSearch: true }))) {
      throw new Error(`RackForge did not publish plugin asset ${url}`);
    }
  }

  for (const request of await cache.keys()) {
    const url = new URL(request.url);
    const marker = url.pathname.indexOf(`/${PLUGIN_ASSET_PREFIX}`);
    const path = marker >= 0
      ? url.pathname.slice(marker + PLUGIN_ASSET_PREFIX.length + 1)
      : "";
    if (!published.has(pluginAssetPath("", path))) {
      await cache.delete(request);
    }
  }
  return published;
}
