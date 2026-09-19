// The typing keyboard: play notes from the computer keys, FL Studio's
// layout -- the Z row is one octave (Z = C3), the Q row the next
// (Q = middle C), sharps on the row above each. Disabled by default and
// enabled from Settings > Input; text fields always win.
export const TYPING_KEYBOARD_STORAGE = "rackforge.typing-keyboard.enabled";

export const TYPING_KEY_NOTES: Record<string, number> = {
  KeyZ: 48, KeyS: 49, KeyX: 50, KeyD: 51, KeyC: 52, KeyV: 53, KeyG: 54,
  KeyB: 55, KeyH: 56, KeyN: 57, KeyJ: 58, KeyM: 59, Comma: 60, KeyL: 61,
  Period: 62, Semicolon: 63, Slash: 64,
  KeyQ: 60, Digit2: 61, KeyW: 62, Digit3: 63, KeyE: 64, KeyR: 65,
  Digit5: 66, KeyT: 67, Digit6: 68, KeyY: 69, Digit7: 70, KeyU: 71,
  KeyI: 72, Digit9: 73, KeyO: 74, Digit0: 75, KeyP: 76, BracketLeft: 77,
  Equal: 78, BracketRight: 79,
};

export function typingKeyboardEnabled(): boolean {
  try {
    return localStorage.getItem(TYPING_KEYBOARD_STORAGE) === "1";
  } catch {
    return false;
  }
}

export function setTypingKeyboardEnabled(enabled: boolean) {
  try {
    localStorage.setItem(TYPING_KEYBOARD_STORAGE, enabled ? "1" : "0");
  } catch {
    /* volatile hosts still toggle for the session */
  }
  window.dispatchEvent(new Event("rackforge:typing-keyboard"));
}
