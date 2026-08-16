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

/** Registered once the page has settled, so it never competes with the boot. */
export function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) return;
  const register = () => {
    void navigator.serviceWorker
      .register(assetUrl("sw.js"), { scope: import.meta.env.BASE_URL })
      .catch((error: unknown) => {
        // Not fatal: without it RackForge simply needs the network to start.
        console.warn("RackForge could not register its offline worker", error);
      });
  };
  if (document.readyState === "complete") {
    register();
  } else {
    window.addEventListener("load", register, { once: true });
  }
}
