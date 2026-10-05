/** Web Audio's historical render quantum, and the sizes offered by browsers
 * that expose variable render quanta. A hint may still be ignored by the UA. */
export const DEFAULT_WEB_BUFFER_FRAMES = 128;
export const PREFERRED_WEB_BUFFER_FRAMES = 256;
export const VARIABLE_WEB_BUFFER_FRAMES = [128, 256, 512] as const;
export const WEB_BUFFER_STORAGE_KEY = "rackforge.web.audio.buffer_frames";

export function webBufferChoices(variableQuanta: boolean): number[] {
  return variableQuanta ? [...VARIABLE_WEB_BUFFER_FRAMES] : [DEFAULT_WEB_BUFFER_FRAMES];
}

export function validWebBufferFrames(value: unknown): value is number {
  return typeof value === "number" && VARIABLE_WEB_BUFFER_FRAMES.some((size) => size === value);
}

export function requestedWebBufferFrames(stored: string | null, variableQuanta: boolean): number {
  if (!variableQuanta) return DEFAULT_WEB_BUFFER_FRAMES;
  const frames = stored === null ? PREFERRED_WEB_BUFFER_FRAMES : Number(stored);
  return validWebBufferFrames(frames) ? frames : PREFERRED_WEB_BUFFER_FRAMES;
}
