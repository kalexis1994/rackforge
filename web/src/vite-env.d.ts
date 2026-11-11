/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * `"1"` when the build carries a RackForge host inside the page instead of
   * talking to one over the network.
   */
  readonly VITE_RACKFORGE_BROWSER_HOST?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

declare const __UI_REVISION__: string;
/** RackForge's version: the workspace's in the root Cargo.toml, "dev" without it. */
declare const __RACKFORGE_VERSION__: string;
