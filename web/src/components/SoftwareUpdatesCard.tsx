import { useCallback, useEffect, useRef, useState } from "react";
import { hostJson, HostRequestError, IS_BROWSER_HOST, isNativeHost } from "../host";
import { updateBusy, updateDescription, type SoftwareUpdateState } from "../softwareUpdates";
import { RfButton } from "../ui/RfButton";
import "./SoftwareUpdatesCard.css";

/** The remote host owns its version and update status, including after a
 * cached browser reconnects. Local app shells keep their own About version. */
export function SoftwareUpdatesCard({ onInformation }: { onInformation?: (state: SoftwareUpdateState) => void }) {
  const [state, setState] = useState<SoftwareUpdateState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const inFlight = useRef(false);
  const supported = !IS_BROWSER_HOST && !isNativeHost();
  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const next = await hostJson<SoftwareUpdateState>("/api/v1/host/software");
      setState(next);
      onInformation?.(next);
      setError(null);
    } catch (error) {
      // Older hosts still serve About, but do not advertise update controls.
      if (error instanceof HostRequestError && error.status === 404) return;
      setError(error instanceof Error ? error.message : "Could not read software information.");
    } finally { inFlight.current = false; }
  }, [onInformation]);
  useEffect(() => {
    if (!supported) return;
    const abort = new AbortController();
    void hostJson<SoftwareUpdateState>("/api/v1/host/software", { signal: abort.signal })
      .then((next) => { if (!abort.signal.aborted) { setState(next); setError(null); onInformation?.(next); } })
      .catch((error: unknown) => {
        if (abort.signal.aborted || (error instanceof HostRequestError && error.status === 404)) return;
        setError(error instanceof Error ? error.message : "Could not read software information.");
      });
    return () => abort.abort();
  }, [supported, onInformation]);
  const busy = checking || (state ? updateBusy(state) : false);
  useEffect(() => {
    if (!supported) return;
    const timer = window.setInterval(() => { void refresh(); }, 1000);
    return () => window.clearInterval(timer);
  }, [supported, refresh]);
  async function check() {
    setChecking(true); setError(null);
    try {
      setState(await hostJson<SoftwareUpdateState>("/api/v1/host/updates/check", { method: "POST" }));
    } catch (error) {
      setError(error instanceof Error ? error.message : "Could not check for updates.");
    } finally { setChecking(false); }
  }
  if (!supported || (!state && !error)) return null;
  return <article className="settings-card" aria-labelledby="software-updates-title">
    <div className="settings-copy">
      <span className="card-kicker">System</span>
      <h2 id="software-updates-title">Software updates</h2>
      {state && <p>Installed host: <strong>RackForge {state.current_version}</strong><br />
        <small className="software-host-build">{state.platform} · build {state.build || "unknown"}</small></p>}
      <p className="software-update-status" role="status" aria-live="polite">{error ?? (state ? updateDescription(state) : "Software information unavailable.")}</p>
      {state?.available?.notes && <p className="software-release-notes">{state.available.notes}</p>}
    </div>
    <RfButton busy={busy} busyLabel={state?.phase === "downloading" ? "Downloading…" : "Checking…"} disabled={state?.phase === "unsupported"} onClick={() => { void check(); }}>
      Check for updates
    </RfButton>
  </article>;
}
