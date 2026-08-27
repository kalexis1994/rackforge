import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AsyncNotice, AsyncStateBoundary } from "./AsyncStateBoundary";

const common = {
  loadingLabel: "Refreshing plugins",
  loadingDetail: "Keeping the catalog synchronized…",
  errorTitle: "Plugin library unavailable",
  errorDetail: "Core did not answer.",
};

describe("AsyncStateBoundary", () => {
  it("shows an initial loader before any valid content exists", () => {
    const markup = renderToStaticMarkup(
      <AsyncStateBoundary {...common} status="loading" hasContent={false}>
        <div>Stale content</div>
      </AsyncStateBoundary>,
    );
    expect(markup).toContain("Refreshing plugins");
    expect(markup).not.toContain("Stale content");
  });

  it("preserves valid content while revalidating", () => {
    const markup = renderToStaticMarkup(
      <AsyncStateBoundary {...common} status="loading" hasContent>
        <div>Concert Grand</div>
      </AsyncStateBoundary>,
    );
    expect(markup).toContain("Concert Grand");
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toContain("Keeping the catalog synchronized…");
  });

  it("offers a retry without discarding stale content after an error", () => {
    const markup = renderToStaticMarkup(
      <AsyncStateBoundary {...common} status="error" hasContent onRetry={vi.fn()}>
        <div>RF-106</div>
      </AsyncStateBoundary>,
    );
    expect(markup).toContain("RF-106");
    expect(markup).toContain("Core did not answer.");
    expect(markup).toContain(">Retry</button>");
  });
});

describe("AsyncNotice", () => {
  it("uses an assertive alert for errors and a polite status for success", () => {
    expect(renderToStaticMarkup(
      <AsyncNotice tone="error" title="Could not activate" />,
    )).toContain('role="alert"');
    expect(renderToStaticMarkup(
      <AsyncNotice tone="success" title="Plugin activated" />,
    )).toContain('role="status"');
  });
});
