export interface PressGesture {
  pointerId: number;
  startX: number;
  startY: number;
  cancelled: boolean;
}

export function beginPressGesture(
  pointerId: number,
  startX: number,
  startY: number,
): PressGesture {
  return { pointerId, startX, startY, cancelled: false };
}

export function updatePressGesture(
  gesture: PressGesture,
  pointerId: number,
  clientX: number,
  clientY: number,
  thresholdPx = 10,
): PressGesture {
  if (gesture.pointerId !== pointerId || gesture.cancelled) return gesture;
  if (Math.hypot(clientX - gesture.startX, clientY - gesture.startY) <= thresholdPx) {
    return gesture;
  }
  return { ...gesture, cancelled: true };
}

export function canCompletePress(gesture: PressGesture, pointerId: number) {
  return gesture.pointerId === pointerId && !gesture.cancelled;
}
