import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { PluginPickerDialog } from "./PluginPickerDialog";
import type { PluginInstance, PluginWebDescriptor } from "../types";

function plugin(
  plugin_id: string,
  plugin_name: string,
  overrides: Partial<PluginWebDescriptor> = {},
): PluginWebDescriptor {
  return {
    plugin_id,
    plugin_name,
    version: "1.0.0",
    kind: "instrument",
    active: true,
    managed: true,
    api_version: 1,
    surfaces: [{ kind: "play", entry_url: "/play" }],
    resources: [],
    ...overrides,
  };
}

function instance(plugin_id: string, plugin_name: string): PluginInstance {
  return {
    instance_id: "rack-slot." + plugin_id,
    plugin_id,
    plugin_name,
    ui_layouts: ["play"],
    config_available: false,
    sounds: [],
  };
}

const catalog = [
  plugin("org.rackforge.piano", "Concert Grand"),
  plugin("org.rackforge.rf-rig", "RF-Rig", { kind: "effect" }),
];

const instances = [
  instance("org.rackforge.piano", "Concert Grand"),
  instance("org.rackforge.rf-rig", "RF-Rig"),
];

function markupFor(role: "instrument" | "effect", available = instances) {
  return renderToStaticMarkup(
    <PluginPickerDialog
      instances={available}
      plugins={catalog}
      role={role}
      onSelect={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe("PluginPickerDialog", () => {
  it("offers only instruments when asked for an instrument", () => {
    const markup = markupFor("instrument");

    expect(markup).toContain("Choose an instrument");
    expect(markup).toContain("Concert Grand");
    expect(markup).not.toContain("RF-Rig");
  });

  it("offers only effects when asked for an effect", () => {
    const markup = markupFor("effect");

    expect(markup).toContain("Choose an effect");
    expect(markup).toContain("RF-Rig");
    expect(markup).not.toContain("Concert Grand");
  });

  it("says where an effect gets its signal from", () => {
    // The node is wired to the hardware input on the player's behalf, so the
    // dialog says so rather than leaving them to find the cable in the graph.
    expect(markupFor("effect")).toContain("wired from the audio input");
  });

  it("names the role it found nothing for", () => {
    const empty = markupFor("effect", [instance("org.rackforge.piano", "Concert Grand")]);

    expect(empty).toContain("No active effects");
    expect(empty).toContain("Activate an effect from Plugin Manager");
  });
});
