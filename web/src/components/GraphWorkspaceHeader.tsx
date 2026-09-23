import { LogOut, Menu, Save } from "lucide-react";
import { AsyncActionLabel } from "./AsyncSpinner";

/**
 * The node editor's own header, across the top of the screen while it is
 * open: what is being edited and whether it is saved, how the preview
 * stands, its name, and Exit and Save. It replaces the thin bar that used to
 * name the Rack and the details dialog a key over the canvas opened -- the
 * details are always in view now, and nothing floats over the graph.
 *
 * Save is off while the graph has an error (rackGraphProblems); the reason is
 * on the key and in the strip under the canvas.
 */
export function GraphWorkspaceHeader({
  title,
  nameLabel,
  name,
  onName,
  previewStatus,
  dirty,
  isNew,
  pending,
  saveBlocked,
  onSave,
  onExit,
  className = "",
}: {
  title: string;
  nameLabel: string;
  name: string;
  onName: (name: string) => void;
  previewStatus: "idle" | "applying" | "ready";
  dirty: boolean;
  isNew: boolean;
  pending: boolean;
  /** Why it cannot be saved as it is, or null. */
  saveBlocked: string | null;
  onSave: () => void;
  onExit: () => void;
  className?: string;
}) {
  return (
    <header className={`graph-workspace-header ${className}`.trim()} aria-label={title}>
      {/* Narrow, where the rail is hidden, the navigation opens from here --
          a key in the header rather than one floating over the canvas. */}
      <button
        type="button"
        className="graph-workspace-menu mobile-menu-button"
        aria-label="Open navigation"
        onClick={() => window.dispatchEvent(new Event("rackforge:open-navigation"))}
      >
        <Menu aria-hidden="true" />
      </button>
      <div className="graph-workspace-title">
        <span className="card-kicker">{title}</span>
        <span className="graph-workspace-state">
          <strong>{dirty || isNew ? "Unsaved changes" : "Saved"}</strong>
          <span className={`graph-workspace-preview ${previewStatus}`} role="status" aria-live="polite">
            {previewStatus === "applying"
              ? "Applying preview…"
              : previewStatus === "ready"
                ? "Preview active"
                : "Preview idle"}
          </span>
        </span>
      </div>
      <label className="graph-workspace-name">
        <span className="visually-hidden">{nameLabel}</span>
        <input
          value={name}
          maxLength={64}
          autoComplete="off"
          onChange={(event) => onName(event.target.value)}
        />
      </label>
      <div className="graph-workspace-actions">
        <button
          type="button"
          className="graph-workspace-key exit"
          disabled={pending}
          onClick={onExit}
        >
          <LogOut aria-hidden="true" />
          <span>Exit</span>
        </button>
        {/* Why Save is off is written on the key it turns off, at its foot,
            taking no width of its own. */}
        <button
          type="button"
          className={`graph-workspace-key save${saveBlocked ? " blocked" : ""}`}
          disabled={(!dirty && !isNew) || pending || !!saveBlocked}
          title={saveBlocked ?? undefined}
          aria-describedby={saveBlocked ? "graph-workspace-blocked-reason" : undefined}
          onClick={onSave}
        >
          <span className="graph-workspace-save-label">
            <AsyncActionLabel active={pending} activeLabel="Saving…">
              <Save aria-hidden="true" />
              <span>Save</span>
            </AsyncActionLabel>
          </span>
          {saveBlocked ? (
            <small className="graph-workspace-blocked">Cannot be saved</small>
          ) : null}
        </button>
        {saveBlocked ? (
          <span id="graph-workspace-blocked-reason" className="visually-hidden">
            {saveBlocked}
          </span>
        ) : null}
      </div>
    </header>
  );
}
