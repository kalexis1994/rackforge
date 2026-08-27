import { describe, expect, it } from "vitest";
import {
  EXPERIENCE_BUDGETS_MS,
  summarizeExperienceMetrics,
  type ExperienceMetric,
} from "./metrics";

function metric(name: ExperienceMetric["name"], duration_ms: number): ExperienceMetric {
  return {
    name,
    duration_ms,
    recorded_at: 0,
    over_budget: duration_ms > EXPERIENCE_BUDGETS_MS[name],
  };
}

describe("experience metrics", () => {
  it("reports p95 and budget violations without mixing metric families", () => {
    const summary = summarizeExperienceMetrics([
      metric("input-feedback", 18),
      metric("input-feedback", 44),
      metric("input-feedback", 61),
      metric("route-ready", 190),
    ]);

    expect(summary.find((item) => item.name === "input-feedback")).toMatchObject({
      samples: 3,
      p95_ms: 61,
      maximum_ms: 61,
      violations: 1,
    });
    expect(summary.find((item) => item.name === "route-ready")).toMatchObject({
      samples: 1,
      p95_ms: 190,
      violations: 0,
    });
  });

  it("keeps metrics with no samples explicit", () => {
    const summary = summarizeExperienceMetrics([]);
    expect(summary).toHaveLength(3);
    expect(summary.every((item) => item.samples === 0 && item.p95_ms === null)).toBe(true);
  });
});
