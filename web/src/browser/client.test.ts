import { describe, expect, it } from "vitest";
import { versionedBrowserAsset } from "./client";

describe("browser host build assets", () => {
  it("versions the storage image with the same revision as the UI", () => {
    expect(versionedBrowserAsset("demo/storage.json"))
      .toBe(`demo/storage.json?v=${encodeURIComponent(__UI_REVISION__)}`);
    expect(versionedBrowserAsset("demo/rackforge/plugins/example/component.wasm"))
      .toBe(
        `demo/rackforge/plugins/example/component.wasm?v=${encodeURIComponent(__UI_REVISION__)}`,
      );
  });

  it("preserves an existing query string", () => {
    expect(versionedBrowserAsset("demo/file.bin?download=1"))
      .toBe(`demo/file.bin?download=1&v=${encodeURIComponent(__UI_REVISION__)}`);
  });
});
