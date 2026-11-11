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

const healthy = [
  status("ready", "Loaded and healthy"),
  status("available", "Active · Loads on demand"),
  status("inactive", "Inactive"),
  status("loading", "Checking runtime…"),
];
const broken = status("unhealthy", "Runtime instance is missing");

describe("PluginRuntimeStatus", () => {
  it("shows every state where a plugin's state is read: the Plugin Manager", () => {
    for (const each of [...healthy, broken]) {
      expect(renderToStaticMarkup(<PluginRuntimeStatus status={each} />)).toContain(each.detail);
    }
  });

  it("says nothing in the PLAY selector while the plugin is fine", () => {
    for (const quiet of healthy) {
      expect(renderToStaticMarkup(<PluginRuntimeStatus status={quiet} problemsOnly />)).toBe("");
    }
    expect(renderToStaticMarkup(<PluginRuntimeStatus problemsOnly />)).toBe("");
  });

  it("names the problem in the PLAY selector when there is one", () => {
    const markup = renderToStaticMarkup(<PluginRuntimeStatus status={broken} problemsOnly />);
    expect(markup).toContain("is-unhealthy");
    expect(markup).toContain("Runtime instance is missing");
  });
});
