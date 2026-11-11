import { describe, expect, it } from "vitest";
import {
  HISTORY_GROUP_MS,
  HISTORY_LIMIT,
  emptyHistory,
  jumpSteps,
  recordStep,
  redoStep,
  undoStep,
} from "./editHistory";

describe("an editor's history", () => {
  it("undoes and redoes a step", () => {
    const history = recordStep(emptyHistory<string>(), "a", "Added", 0);
    const undone = undoStep(history, "b")!;
    expect(undone.value).toBe("a");
    const redone = redoStep(undone.history, undone.value)!;
    expect(redone.value).toBe("b");
    expect(redone.history.past.map((entry) => entry.label)).toEqual(["Added"]);
  });

  it("drops what could be redone when a new step is taken", () => {
    let history = recordStep(emptyHistory<string>(), "a", "One", 0);
    history = undoStep(history, "b")!.history;
    history = recordStep(history, "a", "Two", 5_000);
    expect(history.future).toEqual([]);
    expect(history.past.map((entry) => entry.label)).toEqual(["Two"]);
  });

  it("makes a burst of the same step one step", () => {
    let history = recordStep(emptyHistory<string>(), "", "Renamed Rack", 0);
    history = recordStep(history, "P", "Renamed Rack", HISTORY_GROUP_MS / 2);
    history = recordStep(history, "Pi", "Renamed Rack", HISTORY_GROUP_MS);
    expect(history.past).toHaveLength(1);
    expect(undoStep(history, "Pia")!.value).toBe("");
  });

  it("keeps different steps apart however close", () => {
    let history = recordStep(emptyHistory<string>(), "a", "Added RF-Comp", 0);
    history = recordStep(history, "b", "Moved RF-Comp", 10);
    expect(history.past).toHaveLength(2);
  });

  it("keeps a bounded number of steps", () => {
    let history = emptyHistory<number>();
    for (let step = 0; step < HISTORY_LIMIT + 20; step += 1) {
      history = recordStep(history, step, `Step ${step}`, step * 10_000);
    }
    expect(history.past).toHaveLength(HISTORY_LIMIT);
    expect(history.past[0].value).toBe(20);
  });

  it("jumps several steps either way", () => {
    let history = emptyHistory<number>();
    history = recordStep(history, 0, "One", 0);
    history = recordStep(history, 1, "Two", 10_000);
    history = recordStep(history, 2, "Three", 20_000);
    const back = jumpSteps(history, 3, 2);
    expect(back.value).toBe(1);
    const forward = jumpSteps(back.history, back.value, -2);
    expect(forward.value).toBe(3);
  });
});
