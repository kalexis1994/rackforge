import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RfButton } from "./RfButton";

describe("RfButton", () => {
  it("publishes the shared variant and local feedback contract", () => {
    const markup = renderToStaticMarkup(<RfButton variant="primary">Apply</RfButton>);
    expect(markup).toContain("rf-button--primary");
    expect(markup).toContain('data-rf-press-feedback="local"');
    expect(markup).toContain(">Apply</button>");
  });

  it("becomes unavailable and accessible while busy", () => {
    const markup = renderToStaticMarkup(
      <RfButton busy busyLabel="Installing…">Install</RfButton>,
    );
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toContain("disabled");
    expect(markup).toContain("Installing…");
    expect(markup).not.toContain(">Install</button>");
  });
});
