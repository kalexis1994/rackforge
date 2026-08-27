import { describe, expect, it } from "vitest";
import {
  beginPressGesture,
  canCompletePress,
  updatePressGesture,
} from "./pressGesture";

describe("press gesture", () => {
  it("remains a press inside the movement threshold", () => {
    const gesture = updatePressGesture(beginPressGesture(7, 10, 10), 7, 15, 16);
    expect(canCompletePress(gesture, 7)).toBe(true);
  });

  it("becomes a drag and cannot complete after crossing the threshold", () => {
    const gesture = updatePressGesture(beginPressGesture(7, 10, 10), 7, 30, 10);
    expect(canCompletePress(gesture, 7)).toBe(false);
  });

  it("ignores movement from another pointer", () => {
    const gesture = updatePressGesture(beginPressGesture(7, 10, 10), 8, 100, 100);
    expect(canCompletePress(gesture, 7)).toBe(true);
  });
});
