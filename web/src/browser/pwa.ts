/**
 * Installs RackForge as an application on the device.
 *
 * A page that carries its own host is not really a website: everything it
 * needs — the host, the instrument, the storage — is on the device after the
 * first visit. Registering a service worker lets someone add it to a home
 * screen and open it later with no network at all, which is the state a
 * performer is most likely to be in.
 *
 * Only the self-hosted build does this. A RackForge serving its own interface
 * over the network has a host to reach and should not be answered from a
 * cache.
 */

import { assetUrl } from "../assets";

let registration: Promise<ServiceWorkerRegistration | null> | null = null;

/**
 * Starts the worker exactly once and exposes the same promise to consumers
 * that need it before advertising worker-backed URLs.
 *
 * Registration used to wait for the window `load` event. The plugin catalog
 * can be requested before that event, so its five-second readiness check
 * occasionally expired and permanently cached a descriptor with no branding
 * or web surfaces. Starting here is cheap; installation and cache population
 * still happen asynchronously beside the host boot.
 */
export function ensureServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  if (!("serviceWorker" in navigator)) return Promise.resolve(null);
  if (!registration) {
    registration = navigator.serviceWorker
      .register(assetUrl("sw.js"), { scope: import.meta.env.BASE_URL })
      .catch((error: unknown) => {
        // Not fatal: packaged plugins are served directly and the application
        // itself can still run online. Installed plugin pages need the worker.
        console.warn("RackForge could not register its offline worker", error);
        return null;
      });
  }
  return registration;
}

export function registerServiceWorker() {
  void ensureServiceWorker();
}
