import { describe, expect, it } from "vitest";
import { hostAssetUrl, pluginKitUrls } from "./location";

describe("plugin kit location", () => {
  it("resolves against the app's base, not the page a router is on", () => {
    // BrowserRouter hosts (Desktop, rackforge-web, Android): a plugin's own
    // page is two segments deep, and the kit still lives at the root.
    expect(pluginKitUrls("https://rackforge.local/plugins/play.rf106", "/", false)).toEqual([
      "https://rackforge.local/rackforge-plugin-kit/program-select.js",
    ]);
    expect(hostAssetUrl("rackforge-scrollbars.css", "http://pi:8080/plugins/abc", "/")).toBe(
      "http://pi:8080/rackforge-scrollbars.css",
    );
  });

  it("keeps a sub-path base and a custom scheme", () => {
    expect(pluginKitUrls("https://example.github.io/rackforge/index.html#/play", "/rackforge/", false)).toEqual([
      "https://example.github.io/rackforge/rackforge-plugin-kit/program-select.js",
    ]);
    expect(pluginKitUrls("rackforge://localhost/index.html", "/", false)).toEqual([
      "rackforge://localhost/rackforge-plugin-kit/program-select.js",
    ]);
  });

  it("serves the sources on a dev server", () => {
    expect(pluginKitUrls("http://localhost:5173/plugins/x", "/", true)).toEqual([
      "http://localhost:5173/src/plugin-kit/program-select.ts",
    ]);
  });
});
