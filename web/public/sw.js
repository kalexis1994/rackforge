/**
 * Keeps RackForge working with the network switched off.
 *
 * A performer with a tablet on a stage has no reason to expect a working
 * connection, and RackForge in a page has no reason to need one: the host, the
 * instrument and the storage are all already on the device after the first
 * visit. This worker is what makes the second visit — and every visit after —
 * independent of the network.
 *
 * Three rules, by what the request is for:
 *
 * * navigations go to the network first, so a deployed update is picked up as
 *   soon as it exists, and fall back to the cached page when offline;
 * * build assets carry a content hash in their name, so once cached they are
 *   answered from the cache and never revalidated;
 * * everything else same-origin — the host component, the packaged plugins,
 *   icons and fonts — is answered from the cache and refreshed in the
 *   background, so a visit is instant and the next one is current.
 */

const CACHE = "rackforge-v2";

/**
 * A development server is the one place these rules are exactly wrong.
 *
 * Answering the host component and the packaged plugins from the cache and
 * refreshing them behind the page is what makes a second visit instant; while
 * the thing being served is being rebuilt, it means every reload plays the
 * *previous* build. On a phone there is no cache-bypassing reload to escape
 * with, and the symptom is silent — the instrument simply stays one version
 * behind. So on a local origin everything goes to the network first.
 *
 * The plugin asset cache is exempt and stays cache-only: those files have no
 * server to fall back to, on a dev server least of all.
 */
const DEVELOPMENT =
  ["localhost", "127.0.0.1", "[::1]"].includes(self.location.hostname) ||
  self.location.hostname.endsWith(".local") ||
  // A dev server reached from another device on the same network is still a
  // dev server. It needs a secure context to run at all — a tunnel that makes
  // it localhost, or an origin allow-listed in the browser — and once it has
  // one, the stale-cache trap is exactly the same as on loopback.
  /^10\./.test(self.location.hostname) ||
  /^192\.168\./.test(self.location.hostname) ||
  /^172\.(1[6-9]|2\d|3[01])\./.test(self.location.hostname);
/** Vite writes build output here, with a content hash in every filename. */
const IMMUTABLE = "/assets/";
/**
 * Files that belong to an installed plugin rather than to the site. RackForge
 * writes them here from the host's own storage, which is the only way a page
 * can give a plugin's interface an address to load from.
 */
const PLUGIN_ASSETS = "/plugin-assets/";
const PLUGIN_ASSET_CACHE = "rackforge-plugin-assets";

self.addEventListener("install", (event) => {
  // The page and its manifest are the minimum an offline start needs; the rest
  // arrives as it is requested.
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(["./", "./site.webmanifest"]))
      .then(() => self.skipWaiting())
      .catch(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) =>
        Promise.all(
          names
            .filter((name) => name !== CACHE && name !== PLUGIN_ASSET_CACHE)
            .map((name) => caches.delete(name)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  // Checked before navigations: a plugin's interface is loaded by an iframe,
  // which is itself a navigation, and it must be answered from the package
  // rather than from the site.
  if (url.pathname.includes(PLUGIN_ASSETS)) {
    event.respondWith(pluginAsset(request));
    return;
  }
  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request));
    return;
  }
  if (DEVELOPMENT) {
    // Straight to the network and nothing kept: networkFirst files every
    // response under the scope root, which is right for navigations and
    // would overwrite the cached page with a wasm binary here.
    event.respondWith(fetch(request));
    return;
  }
  if (url.pathname.includes(IMMUTABLE)) {
    event.respondWith(cacheFirst(request));
    return;
  }
  event.respondWith(staleWhileRevalidate(request));
});

async function networkFirst(request) {
  const cache = await caches.open(CACHE);
  try {
    const response = await fetch(request);
    // Navigations are all answered by the same page: store it under the scope
    // root so any route works offline, not only the one that was visited.
    cache.put("./", response.clone());
    return response;
  } catch (error) {
    const cached = (await cache.match(request)) ?? (await cache.match("./"));
    if (cached) return cached;
    throw error;
  }
}

/**
 * Answers from what RackForge published, and from nowhere else: a package
 * file that is not in the cache does not exist on the network either, so a
 * miss is a 404 rather than a request that escapes to the site.
 *
 * The version query is ignored when matching, since it identifies the package
 * version rather than a different file.
 */
async function pluginAsset(request) {
  const cache = await caches.open(PLUGIN_ASSET_CACHE);
  const cached = await cache.match(request, { ignoreSearch: true });
  return (
    cached ??
    new Response("This plugin file is not installed.", {
      status: 404,
      headers: { "content-type": "text/plain; charset=utf-8" },
    })
  );
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const cached = await cache.match(request);
  if (cached) return cached;
  const response = await fetch(request);
  if (response.ok) cache.put(request, response.clone());
  return response;
}

async function staleWhileRevalidate(request) {
  const cache = await caches.open(CACHE);
  const cached = await cache.match(request);
  const network = fetch(request)
    .then((response) => {
      if (response.ok) cache.put(request, response.clone());
      return response;
    })
    .catch((error) => {
      if (cached) return cached;
      throw error;
    });
  return cached ?? network;
}
