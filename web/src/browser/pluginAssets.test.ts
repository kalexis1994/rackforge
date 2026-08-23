import { describe, expect, it } from "vitest";
import {
  canServePluginAssets,
  isPackagedPluginRoot,
  pluginAssetUrl,
} from "./pluginAssets";

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

  it("normalizes Windows package roots before choosing the route", () => {
    expect(isPackagedPluginRoot("plugins\\concert-grand")).toBe(true);
  });
});
