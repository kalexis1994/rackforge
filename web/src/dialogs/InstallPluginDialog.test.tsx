import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PluginInstallPreviewCard, type PluginInstallPreview } from "./InstallPluginDialog";

const preview: PluginInstallPreview = {
  selection_id: "selection",
  plugin_id: "org.example.piano",
  plugin_name: "Example Piano",
  vendor: "Example Audio",
  version: "1.2.3",
  description: null,
  kind: "instrument",
  platform: "wasm32",
  portable: true,
  archive_bytes: 3 * 1024 * 1024,
  branding: null,
};

describe("PluginInstallPreviewCard", () => {
  it("shows the RackForge mark, not a typed monogram, for a package without branding", () => {
    const markup = renderToStaticMarkup(<PluginInstallPreviewCard preview={preview} />);
    expect(markup).toContain("brand-mark");
    expect(markup).not.toContain(">RF<");
    expect(markup).toContain("Instrument by Example Audio, packaged for RackForge.");
    expect(markup).toContain("3.0 MB");
  });

  it("shows a package's own banner, and recolours the card only with what it declares", () => {
    const branded = renderToStaticMarkup(
      <PluginInstallPreviewCard
        preview={{
          ...preview,
          branding: { banner_data_url: "data:image/png;base64,AAAA", accent_color: null },
        }}
      />,
    );
    expect(branded).toContain('src="data:image/png;base64,AAAA"');
    // No accent declared: the faceplate's accent stays, rather than a cyan
    // from the old theme standing in for it.
    expect(branded).not.toContain("--preview-accent");

    const accented = renderToStaticMarkup(
      <PluginInstallPreviewCard
        preview={{
          ...preview,
          branding: { banner_data_url: "data:image/png;base64,AAAA", accent_color: "#c1273d" },
        }}
      />,
    );
    expect(accented).toContain("--preview-accent:#c1273d");
  });
});
