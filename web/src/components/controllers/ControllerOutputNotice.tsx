import type { ControllerOutput } from "../../controllerMapping";
import { AsyncActionLabel } from "../AsyncSpinner";

interface Props {
  output: ControllerOutput;
  /** RackForge ships the package: it sends as installed. */
  shipped: boolean;
  /** This host keeps the answer; elsewhere the notice only informs. */
  canAnswer: boolean;
  busy: boolean;
  onAnswer(allow: boolean): void;
}

function messagesPhrase(output: ControllerOutput) {
  const count = output.messages.length;
  return `${count} ${output.sysex ? "SysEx " : ""}message${count === 1 ? "" : "s"}`;
}

/**
 * What a package asks to send to its controller -- the messages that put a
 * keyboard in the mode the package describes -- and the player's answer.
 * Nothing is sent until they allow it, and a new version of the package
 * asks again.
 */
export function ControllerOutputNotice({ output, shipped, canAnswer, busy, onAnswer }: Props) {
  if (output.state === "none") return null;
  const asked = output.state === "asked";
  return (
    <section className={`controllers-output${asked ? " asked" : ""}`} aria-live="polite">
      <div className="controllers-output-copy">
        <h2>{asked ? "This package asks to talk to the controller" : "Talks to the controller"}</h2>
        <p>
          {asked
            ? `It sends ${messagesPhrase(output)} when the controller connects, usually to put it in the mode this package describes. Nothing is sent until you allow it.`
            : `It sends ${messagesPhrase(output)} when the controller connects${shipped ? ", as every package RackForge ships may" : ""}.`}
        </p>
        <details>
          <summary>The messages</summary>
          <ol>
            {output.messages.map((message, index) => (
              <li key={index}>
                <code>{message}</code>
              </li>
            ))}
          </ol>
        </details>
      </div>
      {canAnswer && !shipped ? (
        <button
          type="button"
          className={asked ? "primary-button" : "secondary-button"}
          disabled={busy}
          onClick={() => onAnswer(asked)}
        >
          <AsyncActionLabel active={busy} activeLabel={asked ? "Allowing…" : "Stopping…"}>
            {asked ? "Allow" : "Stop sending"}
          </AsyncActionLabel>
        </button>
      ) : null}
    </section>
  );
}
