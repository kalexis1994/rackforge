/**
 * Where RackForge serves its plugin kit: the elements its own instruments
 * place in their surfaces. The build writes the kit here, unhashed, and the
 * host injects it into every plugin frame.
 */
export const PLUGIN_KIT_DIRECTORY = "rackforge-plugin-kit";

/** The kit's scripts, by the path the build writes them to. */
export const PLUGIN_KIT_SCRIPTS = ["program-select.js"] as const;

/**
 * A file the build puts at the root of the web UI, as a URL. It is resolved
 * against the app's base (Vite's `BASE_URL`), not the page: under a router
 * with real paths the page can be `/plugins/<id>`, and a path relative to it
 * would ask for `/plugins/<file>`, which the host answers with index.html.
 */
export function hostAssetUrl(path: string, baseUri: string, appBase: string): string {
  const base = appBase.endsWith("/") ? appBase : `${appBase}/`;
  return new URL(`${base}${path}`, baseUri).href;
}

/**
 * The kit's script URLs for a host whose document is at `baseUri` and whose
 * app is built with base `appBase`. A dev server serves the sources themselves.
 */
export function pluginKitUrls(baseUri: string, appBase: string, development: boolean): string[] {
  if (development) return [new URL("/src/plugin-kit/program-select.ts", baseUri).href];
  return PLUGIN_KIT_SCRIPTS.map((script) => hostAssetUrl(`${PLUGIN_KIT_DIRECTORY}/${script}`, baseUri, appBase));
}
