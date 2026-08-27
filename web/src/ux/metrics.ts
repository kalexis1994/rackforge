export const EXPERIENCE_BUDGETS_MS = {
  "input-feedback": 50,
  "route-ready": 250,
  "long-task": 50,
} as const;

export type ExperienceMetricName = keyof typeof EXPERIENCE_BUDGETS_MS;

export interface ExperienceMetric {
  name: ExperienceMetricName;
  duration_ms: number;
  recorded_at: number;
  over_budget: boolean;
}

export interface ExperienceMetricSummary {
  name: ExperienceMetricName;
  budget_ms: number;
  samples: number;
  p95_ms: number | null;
  maximum_ms: number | null;
  violations: number;
}

export interface ExperienceSnapshot {
  revision: number;
  summaries: ExperienceMetricSummary[];
}

const MAX_SAMPLES = 240;
const samples: ExperienceMetric[] = [];
const listeners = new Set<() => void>();
const pendingSpans = new Map<string, number>();
let monitoringStarted = false;
let longTaskObserver: PerformanceObserver | null = null;
let revision = 0;
let snapshot = createSnapshot();

function percentile95(values: number[]): number | null {
  if (values.length === 0) return null;
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[Math.ceil(ordered.length * 0.95) - 1];
}

export function summarizeExperienceMetrics(
  metrics: readonly ExperienceMetric[],
): ExperienceMetricSummary[] {
  return (Object.keys(EXPERIENCE_BUDGETS_MS) as ExperienceMetricName[]).map((name) => {
    const durations = metrics
      .filter((metric) => metric.name === name)
      .map((metric) => metric.duration_ms);
    return {
      name,
      budget_ms: EXPERIENCE_BUDGETS_MS[name],
      samples: durations.length,
      p95_ms: percentile95(durations),
      maximum_ms: durations.length === 0 ? null : Math.max(...durations),
      violations: metrics.filter((metric) => metric.name === name && metric.over_budget).length,
    };
  });
}

function createSnapshot(): ExperienceSnapshot {
  return {
    revision,
    summaries: summarizeExperienceMetrics(samples),
  };
}

export function recordExperienceMetric(
  name: ExperienceMetricName,
  durationMs: number,
) {
  if (!Number.isFinite(durationMs) || durationMs < 0) return;
  samples.push({
    name,
    duration_ms: durationMs,
    recorded_at: Date.now(),
    over_budget: durationMs > EXPERIENCE_BUDGETS_MS[name],
  });
  if (samples.length > MAX_SAMPLES) samples.splice(0, samples.length - MAX_SAMPLES);
  revision += 1;
  snapshot = createSnapshot();
  for (const listener of listeners) listener();
}

export function beginExperienceSpan(name: ExperienceMetricName, key: string = name) {
  pendingSpans.set(key, performance.now());
}

export function completeExperienceSpanAfterPaint(
  name: ExperienceMetricName,
  key: string = name,
) {
  const startedAt = pendingSpans.get(key);
  if (startedAt === undefined) return;
  pendingSpans.delete(key);
  measureNextPaint(name, startedAt);
}

export function measureNextPaint(
  name: ExperienceMetricName,
  startedAt = performance.now(),
) {
  if (typeof requestAnimationFrame !== "function") {
    recordExperienceMetric(name, performance.now() - startedAt);
    return;
  }
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      recordExperienceMetric(name, performance.now() - startedAt);
    });
  });
}

export function subscribeExperienceMetrics(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getExperienceSnapshot() {
  return snapshot;
}

export function startExperienceMonitoring() {
  if (monitoringStarted || typeof PerformanceObserver === "undefined") return;
  monitoringStarted = true;
  try {
    longTaskObserver = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        recordExperienceMetric("long-task", entry.duration);
      }
    });
    longTaskObserver.observe({ type: "longtask", buffered: true });
  } catch {
    longTaskObserver = null;
    // Older WebViews do not expose Long Tasks. Interaction metrics still work.
  }
}
