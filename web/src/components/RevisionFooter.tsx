import { useHostHealth } from "../hooks/useHostHealth";

// The version a player knows RackForge by, under the navigation. The builds
// behind it -- the revision this interface was built from, beside the one
// the host binary reports -- are in its title and on About. When those
// disagree, someone shipped half a deploy, and the footer says so before a
// behavior difference does.
export function RevisionFooter() {
  const host = useHostHealth();
  const mismatch =
    host?.ui_revision !== undefined &&
    host.ui_revision !== "unknown" &&
    host.ui_revision !== __UI_REVISION__;
  const stale = host?.revision !== undefined && host.revision !== __UI_REVISION__;
  const builds = `UI ${__UI_REVISION__}${host?.revision ? ` · host ${host.revision}` : ""}`;
  return (
    <p className={`revision-footer${mismatch || stale ? " drift" : ""}`} title={builds}>
      v{__RACKFORGE_VERSION__}
      {mismatch || stale ? ` · out of sync (${builds})` : ""}
    </p>
  );
}
