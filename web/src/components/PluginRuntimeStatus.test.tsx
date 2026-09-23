import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PluginRuntimeStatus as RuntimeStatus } from "../pluginCatalog";
import { PluginRuntimeStatus } from "./PluginRuntimeStatus";

const status = (phase: RuntimeStatus["phase"], detail: string): RuntimeStatus => ({
  plugin_id: "org.example.piano",
  phase,
  loaded: phase === "ready",
  healthy: phase === "ready" ? true : phase === "unhealthy" ? false : null,
  detail,
});

describe("PluginRuntimeStatus", () => {
  it("says nothing while the plugin is fine", () => {
    for (const quiet of [
      status("ready", "Loaded and healthy"),
      status("available", "Active · Loads on demand"),
      status("inactive", "Inactive"),
      status("loading", "Checking runtime…"),
    ]) {
      expect(renderToStaticMarkup(<PluginRuntimeStatus status={quiet} />)).toBe("");
    }
    expect(renderToStaticMarkup(<PluginRuntimeStatus />)).toBe("");
  });

  it("names the problem when there is one", () => {
    const markup = renderToStaticMarkup(
      <PluginRuntimeStatus status={status("unhealthy", "Runtime instance is missing")} />,
    );
    expect(markup).toContain("is-unhealthy");
    expect(markup).toContain("Runtime instance is missing");
  });
});
