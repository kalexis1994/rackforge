import { describe, expect, it } from "vitest";
import { isPageDownload } from "./downloads";

describe("plugin page downloads", () => {
  it("saves only named links to the page's own bytes", () => {
    expect(isPageDownload({ download: "RF-5 40 programs.wav", href: "blob:https://host/3f2a" })).toBe(true);
    expect(isPageDownload({ download: "preset.json", href: "data:application/json,%7B%7D" })).toBe(true);
    expect(isPageDownload({ download: "", href: "blob:https://host/3f2a" })).toBe(false);
    expect(isPageDownload({ download: "manual.pdf", href: "https://example.com/manual.pdf" })).toBe(false);
  });
});
