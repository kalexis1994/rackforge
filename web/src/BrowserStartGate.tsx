import { type ReactNode, useEffect, useState } from "react";
import { browserAudioNeedsGesture, startBrowserHost } from "./browser/client";
import { BrandMark } from "./components/BrandMark";
import { RfLoader } from "./components/RfLoader";

/** Keep the browser engine behind a real gesture, before the app starts loading. */
export function BrowserStartGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<"idle" | "starting" | "ready">("idle");
  const [error, setError] = useState<string | null>(null);
  const [needsGesture, setNeedsGesture] = useState(false);

  useEffect(() => {
    if (state !== "starting") return;
    const update = () => setNeedsGesture(browserAudioNeedsGesture());
    update();
    const timer = window.setInterval(update, 500);
    return () => window.clearInterval(timer);
  }, [state]);

  const start = () => {
    setError(null);
    setState("starting");
    // The AudioContext is created synchronously by this call, in the button's
    // activation. A second tap can resume that same context while it boots.
    void startBrowserHost()
      .then(() => setState("ready"))
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : "RackForge could not start.");
        setState("idle");
      });
  };

  if (state === "ready") return <>{children}</>;

  return (
    <main className="browser-start-gate pairing-shell">
      {state === "starting" ? (
        <RfLoader label="Starting RackForge" detail="Loading the audio engine and plugins…" size="large" />
      ) : (
        <div className="browser-start-gate__brand" aria-label="RackForge">
          <BrandMark />
          <span>RACKFORGE</span>
        </div>
      )}
      {state === "idle" || needsGesture ? (
        <button type="button" className="primary-button" onClick={start}>
          {state === "idle" ? "Start RackForge" : "Enable audio"}
        </button>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </main>
  );
}
