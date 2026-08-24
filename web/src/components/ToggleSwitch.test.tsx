import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ToggleSwitch } from "./ToggleSwitch";

describe("ToggleSwitch", () => {
  it("exposes switch semantics and a clear enabled state", () => {
    const markup = renderToStaticMarkup(
      <ToggleSwitch
        checked
        label="Typing input"
        description="Computer keys play notes"
        checkedLabel="Enabled"
        uncheckedLabel="Disabled"
        onChange={vi.fn()}
      />,
    );

    expect(markup).toContain('role="switch"');
    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain("Typing input");
    expect(markup).toContain("Computer keys play notes");
    expect(markup).toContain("Enabled");
    expect(markup).not.toContain('type="checkbox"');
  });

  it("renders a disabled off state", () => {
    const markup = renderToStaticMarkup(
      <ToggleSwitch checked={false} label="Feature" disabled onChange={vi.fn()} />,
    );

    expect(markup).toContain('aria-checked="false"');
    expect(markup).toContain("disabled");
    expect(markup).toContain("Off");
  });
});
