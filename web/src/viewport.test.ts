import { describe, expect, it } from "vitest";
import { withoutZoom } from "./viewport";

describe("viewport in a native shell", () => {
  it("keeps the page's own terms and fixes the scale", () => {
    expect(withoutZoom("width=device-width, initial-scale=1.0, viewport-fit=cover")).toBe(
      "width=device-width, initial-scale=1.0, viewport-fit=cover, maximum-scale=1, minimum-scale=1, user-scalable=no",
    );
  });

  it("replaces scaling terms already there instead of doubling them", () => {
    expect(withoutZoom("width=device-width,user-scalable=yes, maximum-scale=5")).toBe(
      "width=device-width, maximum-scale=1, minimum-scale=1, user-scalable=no",
    );
    expect(withoutZoom("")).toBe("maximum-scale=1, minimum-scale=1, user-scalable=no");
  });
});
