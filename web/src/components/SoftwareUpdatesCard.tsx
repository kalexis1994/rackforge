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
  const [confirming, setConfirming] = useState(false);
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
    if (state?.phase === "installed" && state.build && !state.build.startsWith(__UI_REVISION__)) {
      const identity = `${state.current_version}:${state.build}`;
      if (sessionStorage.getItem("rackforge-updated-interface") !== identity) {
        sessionStorage.setItem("rackforge-updated-interface", identity);
        window.location.reload();
      }
    }
  }, [state?.phase, state?.build, state?.current_version]);
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
  async function install(recover = false) {
    if (!state || (!recover && !state.available)) return;
    setChecking(true); setConfirming(false); setError(null);
    try {
      const next = await hostJson<SoftwareUpdateState>(`/api/v1/host/updates/${recover ? "recover" : "install"}`, {
        method: "POST", ...(recover ? {} : { body: JSON.stringify({ version: state.available!.version }) }),
      });
      setState(next);
    } catch (error) {
      setError(error instanceof Error ? error.message : "Could not start the installation.");
    } finally { setChecking(false); }
  }
  if (!supported || (!state && !error)) return null;
  return <article className="settings-card" aria-labelledby="software-updates-title">
    <div className="settings-copy">
      <span className="card-kicker">System</span>
      <h2 id="software-updates-title">Software updates</h2>
      {state && <p>Installed host: <strong>RackForge {state.current_version}</strong><br />
        <small className="software-host-build">{state.platform} · build {state.build || "unknown"}</small></p>}
      <p className="software-update-status" role="status" aria-live="polite">{state && ["installing", "recovering"].includes(state.phase) ? updateDescription(state) : error ?? (state ? updateDescription(state) : "Software information unavailable.")}</p>
      {state?.available?.notes && <p className="software-release-notes">{state.available.notes}</p>}
    </div>
    <div className="software-update-actions">
    <RfButton busy={busy} busyLabel={state?.phase === "downloading" ? "Downloading…" : ["installing", "recovering"].includes(state?.phase ?? "") ? "Restarting…" : "Checking…"} disabled={state?.phase === "unsupported" || confirming} onClick={() => { void check(); }}>
      Check for updates
    </RfButton>
    {state?.can_install && state.available && ["available", "ready"].includes(state.phase) && !confirming && <RfButton variant="primary" disabled={busy} onClick={() => setConfirming(true)}>Install and restart</RfButton>}
    {confirming && <div role="group" aria-label="Confirm installation">
      <p>Install RackForge {state?.available?.version}? Audio and the controller will stop briefly. The previous installation will be restored if the update fails.</p>
      <RfButton variant="primary" disabled={busy || !state?.available || !["available", "ready"].includes(state.phase)} onClick={() => { void install(); }}>Install now</RfButton>
      <RfButton disabled={busy} onClick={() => setConfirming(false)}>Cancel</RfButton>
    </div>}
    {state?.phase === "recovery_failed" && <RfButton disabled={busy} onClick={() => { void install(true); }}>Retry recovery</RfButton>}
    </div>
  </article>;
}
