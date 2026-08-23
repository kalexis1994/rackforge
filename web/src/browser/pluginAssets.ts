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
export function whenServing(timeoutMs = 5_000): Promise<boolean> {
  if (!("serviceWorker" in navigator)) return Promise.resolve(false);
  if (navigator.serviceWorker.controller) return Promise.resolve(true);
  return new Promise((resolve) => {
    const settle = (serving: boolean) => {
      navigator.serviceWorker.removeEventListener("controllerchange", onChange);
      window.clearTimeout(timer);
      resolve(serving);
    };
    const onChange = () => settle(Boolean(navigator.serviceWorker.controller));
    const timer = window.setTimeout(() => settle(false), timeoutMs);
    navigator.serviceWorker.addEventListener("controllerchange", onChange);
    // Do not merely wait and hope another part of the application registers
    // the worker. The catalog itself depends on it for installed packages.
    void ensureServiceWorker().then(() => navigator.serviceWorker.ready).then(() => {
      if (navigator.serviceWorker.controller) settle(true);
    }).catch(() => settle(false));
  });
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

/**
 * Republishes every packaged file the host currently holds.
 *
 * Called with the same storage snapshot the page files in IndexedDB, so the
 * published files and the installed packages can never disagree. Entries no
 * longer present are dropped, which is what makes an uninstall take effect.
 */
export async function publishPluginAssets(files: SeedFile[]): Promise<void> {
  if (!("caches" in globalThis)) return;
  const cache = await caches.open(CACHE);
  const published = new Set<string>();

  for (const file of files) {
    // Only package contents are published; the host's private storage —
    // presets, plugin state, the performance library — stays private.
    if (!file.path.startsWith("plugins/") && !file.path.startsWith("store/packages/")) {
      continue;
    }
    const url = `${assetUrl(PLUGIN_ASSET_PREFIX)}${file.path}`;
    published.add(new URL(url, location.href).pathname);
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

  for (const request of await cache.keys()) {
    if (!published.has(new URL(request.url).pathname)) {
      await cache.delete(request);
    }
  }
}
