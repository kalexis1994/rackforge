import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ModalDialog } from "./ModalDialog";

describe("ModalDialog", () => {
  it("provides one accessible shell for titles, messages and actions", () => {
    const markup = renderToStaticMarkup(
      <ModalDialog
        eyebrow="Portable package"
        title="Install plugin"
        message={<p>Review this package before installation.</p>}
        actions={<button type="button">Install</button>}
        onClose={vi.fn()}
        closeLabel="Close plugin installer"
      >
        <div>Package preview</div>
      </ModalDialog>,
    );

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain("Portable package");
    expect(markup).toContain("Install plugin");
    expect(markup).toContain("Review this package before installation.");
    expect(markup).toContain("Package preview");
    expect(markup).toContain(">Install</button>");
    expect(markup).toContain('aria-label="Close plugin installer"');
  });

  it("can replace dismissal with an explicit cancellation action", () => {
    const markup = renderToStaticMarkup(
      <ModalDialog
        title="Installing plugin"
        onClose={vi.fn()}
        dismissible={false}
        showClose={false}
        actions={<button type="button">Cancel installation</button>}
      />,
    );

    expect(markup).not.toContain("modal-dialog-close");
    expect(markup).toContain("Cancel installation");
  });
});
