import { useEffect, useState } from "react";
import { sendVirtualMidi } from "../gateway";
import { TYPING_KEY_NOTES, typingKeyboardEnabled } from "../typingKeyboard";

export function TypingKeyboardListener() {
  const [enabled, setEnabled] = useState(typingKeyboardEnabled);
  useEffect(() => {
    const sync = () => setEnabled(typingKeyboardEnabled());
    window.addEventListener("rackforge:typing-keyboard", sync);
    return () => window.removeEventListener("rackforge:typing-keyboard", sync);
  }, []);
  useEffect(() => {
    if (!enabled) return;
    const held = new Set<number>();
    const isTextTarget = (target: EventTarget | null) => {
      const element = target as HTMLElement | null;
      if (!element) return false;
      const tag = element.tagName;
      if (tag === "TEXTAREA" || tag === "SELECT") return true;
      if (element.isContentEditable) return true;
      if (tag === "INPUT") {
        // Only TEXT entry wins over the notes; a focused fader, checkbox
        // or button is not typing.
        const type = (element as HTMLInputElement).type;
        return !["checkbox", "radio", "range", "button", "color"].includes(type);
      }
      return false;
    };
    const down = (event: KeyboardEvent) => {
      if (event.repeat || event.ctrlKey || event.metaKey || event.altKey) return;
      if (isTextTarget(event.target)) return;
      const note = TYPING_KEY_NOTES[event.code];
      if (note === undefined || held.has(note)) return;
      event.preventDefault();
      held.add(note);
      sendVirtualMidi(0x90, note, 100);
    };
    const up = (event: KeyboardEvent) => {
      const note = TYPING_KEY_NOTES[event.code];
      if (note === undefined || !held.has(note)) return;
      held.delete(note);
      sendVirtualMidi(0x80, note, 0);
    };
    const releaseAll = () => {
      for (const note of held) sendVirtualMidi(0x80, note, 0);
      held.clear();
    };
    // Keyboard events do not cross frame boundaries, and in Play mode the
    // player's focus usually sits inside the plugin panel's iframe -- so
    // the listener rides along into every same-origin frame, re-scanned as
    // panels mount and unmount.
    const attached = new Set<Window>();
    const attach = (target: Window) => {
      if (attached.has(target)) return;
      attached.add(target);
      target.addEventListener("keydown", down);
      target.addEventListener("keyup", up);
    };
    attach(window);
    const scanFrames = () => {
      for (const frame of Array.from(document.querySelectorAll("iframe"))) {
        try {
          const inner = (frame as HTMLIFrameElement).contentWindow;
          if (inner && inner.document) attach(inner);
        } catch {
          /* cross-origin frames keep their keys */
        }
      }
    };
    scanFrames();
    const scanner = window.setInterval(scanFrames, 2000);
    window.addEventListener("blur", releaseAll);
    return () => {
      window.clearInterval(scanner);
      for (const target of attached) {
        try {
          target.removeEventListener("keydown", down);
          target.removeEventListener("keyup", up);
        } catch {
          /* a navigated-away frame is already gone */
        }
      }
      window.removeEventListener("blur", releaseAll);
      releaseAll();
    };
  }, [enabled]);
  return null;
}
