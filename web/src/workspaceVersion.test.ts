import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { workspaceVersion } from "./workspaceVersion";

describe("workspace version", () => {
  it("reads the version of [workspace.package], not another section's", () => {
    const toml = [
      "[workspace]",
      'members = ["crates/*"]',
      "",
      "[workspace.dependencies]",
      'serde = { version = "1.0" }',
      "",
      "[workspace.package]",
      'edition = "2024"',
      'version = "0.1.27"',
      "",
      "[profile.release]",
      'version = "not this"',
    ].join("\r\n");
    expect(workspaceVersion(toml)).toBe("0.1.27");
    expect(workspaceVersion('[package]\nversion = "9.9.9"\n')).toBeNull();
  });

  it("is what the interface is built with", () => {
    const toml = readFileSync(join(__dirname, "..", "..", "Cargo.toml"), "utf8");
    expect(__RACKFORGE_VERSION__).toBe(workspaceVersion(toml));
  });
});
