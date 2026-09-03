/**
 * The screen a machine shows once: RackForge getting itself ready.
 *
 * It is the opening half of the closing screen the desktop shows when it
 * quits — the same plate, the same list of steps ticking over — with the
 * mark RackForge already waits behind lighting up beside the heading.
 *
 * When something fails it offers the thing that would fix it, and never
 * "continue": an interface with no instrument in it is the dead end this
 * screen exists to end, so walking the player into it deliberately would be
 * the one useless button on the panel. A machine with no instruments is sent
 * to Plugin Manager, where instruments come from; everything else is asked
 * of the host again.
 */

import { RfLoader } from "./components/RfLoader";
import type { FirstRunFailure, FirstRunStep, FirstRunView } from "./firstRun";

function StepMark({ state }: { state: FirstRunStep["state"] }) {
  if (state === "done") {
    return (
      <span className="first-run-mark first-run-mark--done" aria-hidden="true">
        <svg viewBox="0 0 12 12" width="12" height="12">
          <path
            d="M2 6.4 4.6 9 10 3.2"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
    );
  }
  if (state === "failed") {
    return (
      <span className="first-run-mark first-run-mark--failed" aria-hidden="true">
        <svg viewBox="0 0 12 12" width="12" height="12">
          <path
            d="M3 3 9 9M9 3 3 9"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
          />
        </svg>
      </span>
    );
  }
  return (
    <span
      className={`first-run-mark first-run-mark--${state === "working" ? "working" : "waiting"}`}
      aria-hidden="true"
    />
  );
}

export function FirstRunScreen({
  view,
  failure,
  onRetry,
  onOpenPluginManager,
}: {
  view: FirstRunView;
  failure: FirstRunFailure | null;
  onRetry: () => void;
  onOpenPluginManager: () => void;
}) {
  return (
    <div
      className="first-run-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="Preparing RackForge"
    >
      <section className="first-run-plate">
        <header className="first-run-head">
          <RfLoader className="first-run-loader" size="compact" label="" />
          <div>
            <h1>PREPARING RACKFORGE</h1>
            <p>{failure ? "Some of this did not finish" : "Setting up your instruments"}</p>
          </div>
        </header>
        <div
          className="first-run-bar"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(view.progress * 100)}
        >
          <span style={{ width: `${Math.round(view.progress * 100)}%` }} />
        </div>
        <ol className="first-run-steps">
          {view.steps.map((step) => (
            <li key={step.id} className={`first-run-step first-run-step--${step.state}`}>
              <StepMark state={step.state} />
              <span className="first-run-step-label">{step.label}</span>
              {step.detail ? <span className="first-run-step-detail">{step.detail}</span> : null}
            </li>
          ))}
        </ol>
        {failure ? (
          <footer className="first-run-foot">
            <p>{failure.message}</p>
            {failure.kind === "no_instruments" ? (
              <button type="button" onClick={onOpenPluginManager}>
                Open Plugin Manager
              </button>
            ) : (
              <button type="button" onClick={onRetry}>
                Try again
              </button>
            )}
          </footer>
        ) : null}
      </section>
    </div>
  );
}
