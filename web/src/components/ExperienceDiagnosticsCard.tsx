import { useSyncExternalStore } from "react";
import {
  getExperienceSnapshot,
  subscribeExperienceMetrics,
} from "../ux/metrics";

const METRIC_LABELS = {
  "input-feedback": "Input feedback",
  "route-ready": "Warm navigation",
  "long-task": "Main-thread task",
} as const;

function milliseconds(value: number | null) {
  return value === null ? "Waiting for samples" : `${Math.round(value)} ms p95`;
}

export function ExperienceDiagnosticsCard() {
  const snapshot = useSyncExternalStore(
    subscribeExperienceMetrics,
    getExperienceSnapshot,
    getExperienceSnapshot,
  );
  const violations = snapshot.summaries.reduce((total, item) => total + item.violations, 0);

  return (
    <article className="settings-card experience-diagnostics-card">
      <div className="settings-copy">
        <span className="card-kicker">Local interface · live samples</span>
        <h2>Experience budgets</h2>
        <p>
          {violations === 0
            ? "Observed interactions are within their current budgets."
            : `${violations} observed sample${violations === 1 ? " is" : "s are"} over budget.`}
        </p>
      </div>
      <dl className="settings-values">
        {snapshot.summaries.map((summary) => (
          <div key={summary.name} className={summary.violations > 0 ? "over-budget" : ""}>
            <dt>{METRIC_LABELS[summary.name]}</dt>
            <dd>{milliseconds(summary.p95_ms)}</dd>
            <small>
              Budget {summary.budget_ms} ms · {summary.samples} sample{summary.samples === 1 ? "" : "s"}
            </small>
          </div>
        ))}
      </dl>
    </article>
  );
}
