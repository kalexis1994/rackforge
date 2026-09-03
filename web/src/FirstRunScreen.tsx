/**
 * The screen a machine shows once: RackForge getting itself ready.
 *
 * It is the opening half of the closing screen the desktop shows when it
 * quits — the same plate, the same monogram, the same list of steps ticking
 * over — because they are the same moment seen from the two ends.
 */

import { RfLoader } from "./components/RfLoader";
import type { FirstRunStep, FirstRunView } from "./firstRun";

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
  onDismiss,
}: {
  view: FirstRunView;
  failure: string | null;
  onDismiss: () => void;
}) {
  return (
    <div className="first-run-backdrop" role="dialog" aria-modal="true" aria-label="Preparing RackForge">
      <section className="first-run-plate">
        <header className="first-run-head">
          {/* The mark RackForge already waits behind, lit limb by limb: this
              screen is a wait like any other, and it should look like one.
              Its own label is hidden -- the heading beside it is the label. */}
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
            <p>{failure}</p>
            {/* Never a trap: whatever failed, the interface is one press away. */}
            <button type="button" onClick={onDismiss}>
              Continue to RackForge
            </button>
          </footer>
        ) : null}
      </section>
    </div>
  );
}
