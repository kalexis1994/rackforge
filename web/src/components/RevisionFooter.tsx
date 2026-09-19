import { useHostHealth } from "../hooks/useHostHealth";

// Drift made visible: the revision this interface was built from, beside
// the revision the host binary reports. When they disagree, someone shipped
// half a deploy, and the mismatch says so before a behavior difference does.
export function RevisionFooter() {
  const host = useHostHealth();
  const mismatch =
    host?.ui_revision !== undefined &&
    host.ui_revision !== "unknown" &&
    host.ui_revision !== __UI_REVISION__;
  const stale = host?.revision !== undefined && host.revision !== __UI_REVISION__;
  return (
    <p className={`revision-footer${mismatch || stale ? " drift" : ""}`}>
      UI {__UI_REVISION__}
      {host?.revision ? ` · host ${host.revision}` : ""}
      {mismatch || stale ? " · out of sync" : ""}
    </p>
  );
}
