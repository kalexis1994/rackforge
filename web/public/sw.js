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

const CACHE = "rackforge-v1";
/** Vite writes build output here, with a content hash in every filename. */
const IMMUTABLE = "/assets/";

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
        Promise.all(names.filter((name) => name !== CACHE).map((name) => caches.delete(name))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request));
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
