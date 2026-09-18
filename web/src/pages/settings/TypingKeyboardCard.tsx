import { useState } from "react";
import { ToggleSwitch } from "../../components/ToggleSwitch";
import { setTypingKeyboardEnabled, typingKeyboardEnabled } from "../../typingKeyboard";
import { Keyboard, } from "lucide-react";

export function TypingKeyboardCard() {
  const [enabled, setEnabled] = useState(typingKeyboardEnabled);
  const toggle = (next: boolean) => {
    setEnabled(next);
    setTypingKeyboardEnabled(next);
  };
  return (
    <article className="settings-card">
      <div className="settings-icon settings-icon-svg">
        <Keyboard aria-hidden="true" strokeWidth={1.7} />
      </div>
      <div className="settings-copy">
        <span className="card-kicker">Computer keys</span>
        <h2>Typing Keyboard</h2>
        <p>
          Play notes with the computer keyboard, FL Studio layout: the Z row
          is one octave, the Q row the next, sharps on the row above each.
          Text fields always take priority.
        </p>
      </div>
      <ToggleSwitch
        className="typing-keyboard-switch"
        checked={enabled}
        label="Typing input"
        description={enabled ? "Computer keys play notes" : "Computer keys are ignored"}
        checkedLabel="Enabled"
        uncheckedLabel="Disabled"
        onChange={toggle}
      />
    </article>
  );
}
