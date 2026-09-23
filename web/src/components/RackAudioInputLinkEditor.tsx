import { useId, useState } from "react";
import { useCanvasModal } from "../hooks/useCanvasModal";
import type { RackAudioInputRoute, RackGraphEdge } from "../types";
import type { AudioInputPeakFeed, AudioInputState } from "../hooks/useAudioInputStatus";
import { AudioInputMeter } from "./AudioInputMeter";
import { ScrubNumberField } from "./ScrubNumberField";
import { formatInputList } from "../rackGraph";

/** The trim a cable may apply, in dB -- the host's own input trim's range. */
export const AUDIO_INPUT_ROUTE_GAIN_MIN_DB = -60;
export const AUDIO_INPUT_ROUTE_GAIN_MAX_DB = 24;
/** The highest input number a cable may name (the host's limit). */
const MAX_INPUT = 64;

interface RackAudioInputLinkEditorProps {
  edge: RackGraphEdge;
  targetLabel?: string;
  status: AudioInputState | null;
  peaks: AudioInputPeakFeed | null;
  /** The route to keep on the cable, or undefined for none: every captured
   *  input at unity, which is what a cable without one carries. */
  onApply: (route: RackAudioInputRoute | undefined) => void;
  onClose: () => void;
}

/** A route as it is kept: nothing for the default, channels only when some
 *  are chosen, a trim only when it is not unity. */
export function normalizeAudioInputRoute(
  route: RackAudioInputRoute,
): RackAudioInputRoute | undefined {
  const channels = (route.channels ?? []).slice(0, 2);
  const gain = Math.round(Math.max(
    AUDIO_INPUT_ROUTE_GAIN_MIN_DB,
    Math.min(AUDIO_INPUT_ROUTE_GAIN_MAX_DB, route.gain_db ?? 0),
  ));
  if (channels.length === 0 && gain === 0) return undefined;
  return {
    ...(channels.length > 0 ? { channels } : {}),
    ...(gain !== 0 ? { gain_db: gain } : {}),
  };
}

/** How many inputs to offer: the interface's, and never fewer than what is
 *  captured or already chosen, so a route saved on another machine still
 *  shows what it names. */
export function offeredInputCount(
  status: AudioInputState | null,
  route: RackAudioInputRoute,
): number {
  const highest = Math.max(
    2,
    status?.device_channels ?? 0,
    ...(status?.captured ?? []),
    ...(route.channels ?? []),
  );
  return Math.min(MAX_INPUT, highest);
}

function sameChannels(a: number[], b: number[]) {
  return a.length === b.length && a.every((channel, index) => channel === b[index]);
}

function statusLine(status: AudioInputState | null): { tone: "note" | "warning"; text: string } {
  if (!status) {
    return { tone: "note", text: "This host has not said what it captures." };
  }
  switch (status.availability) {
    case "unsupported":
      return { tone: "warning", text: "This host has no audio input. The cable carries silence here." };
    case "disabled":
      return { tone: "warning", text: "This host has no audio input selected. Choose one in its audio settings." };
    case "absent":
      return {
        tone: "warning",
        text: `${status.device_name ?? "The audio input"} is not available${status.reason ? `: ${status.reason}` : "."}`,
      };
    case "open":
      break;
  }
  const captured = status.captured.length > 0
    ? `captures input${status.captured.length > 1 ? "s" : ""} ${formatInputList(status.captured)}`
    : "captures nothing";
  const trim = status.gain_db !== 0 ? ` at ${status.gain_db > 0 ? "+" : ""}${status.gain_db} dB` : "";
  return {
    tone: "note",
    text: `${status.device_name ?? "The interface"} ${captured}${trim}.`,
  };
}

/**
 * The settings of a cable from the Rack's audio input: which of the
 * interface's inputs it carries -- all of them, one as a mono source, or a
 * pair as stereo -- and its trim, with the level of what it carries.
 */
export function RackAudioInputLinkEditor({
  edge,
  targetLabel = "Plugin",
  status,
  peaks,
  onApply,
  onClose,
}: RackAudioInputLinkEditorProps) {
  const { sectionRef, closeRef, onKeyDown } = useCanvasModal(onClose);
  const titleId = useId();
  const [draft, setDraft] = useState<RackAudioInputRoute>(() => ({
    channels: edge.audio_input_route?.channels ?? [],
    gain_db: edge.audio_input_route?.gain_db ?? 0,
  }));
  const channels = draft.channels ?? [];
  const gainDb = draft.gain_db ?? 0;
  const count = offeredInputCount(status, draft);
  const captured = new Set(status?.availability === "open" ? status.captured : []);
  const knowsCapture = status?.availability === "open";
  const inputs = Array.from({ length: count }, (_, index) => index + 1);
  const pairs = inputs.filter((input) => input % 2 === 1 && input + 1 <= count)
    .map((left) => [left, left + 1]);
  // A pair chosen elsewhere that is not one of the interface's own (2–3, or
  // one swapped) is still offered, as it is, so it can be seen and kept.
  if (channels.length === 2 && !pairs.some((pair) => sameChannels(pair, [...channels].sort((a, b) => a - b)))) {
    pairs.push([...channels].sort((a, b) => a - b));
  }
  const choose = (next: number[]) => setDraft((current) => ({ ...current, channels: next }));
  const missing = channels.filter((channel) => knowsCapture && !captured.has(channel));
  const line = statusLine(status);
  const meterInputs = channels.length > 0 ? channels : status?.captured ?? [];

  const keyClass = (active: boolean, inputsOfKey: number[]) => [
    active ? "active" : "",
    knowsCapture && inputsOfKey.some((input) => !captured.has(input)) ? "is-not-captured" : "",
  ].filter(Boolean).join(" ");
  const keyTitle = (inputsOfKey: number[]) => knowsCapture && inputsOfKey.some((input) => !captured.has(input))
    ? "Not captured by this host"
    : undefined;

  return (
    <>
    <div className="rack-link-editor-scrim" aria-hidden="true" onPointerDown={(event) => event.stopPropagation()} />
    <section
      ref={sectionRef}
      className="rack-midi-link-editor rack-audio-link-editor"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onKeyDown={onKeyDown}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <header>
        <div>
          <span>AUDIO CONNECTION</span>
          <strong id={titleId}>Audio input → {targetLabel}</strong>
        </div>
        <button ref={closeRef} type="button" className="rack-midi-close" aria-label="Close" title="Close (Esc)" onClick={onClose}>×</button>
      </header>
      <div className="rack-midi-link-scroll">
        <section className="rack-midi-section">
          <h3>Inputs</h3>
          <p className={`rack-audio-link-status ${line.tone}`} role={line.tone === "warning" ? "alert" : undefined}>
            {line.text}
          </p>
          <fieldset className="rack-audio-input-picker">
            <legend>Source</legend>
            <div>
              <button
                type="button"
                className={channels.length === 0 ? "active" : ""}
                aria-pressed={channels.length === 0}
                onClick={() => choose([])}
                title="Every input the host captures"
              >
                All
              </button>
            </div>
          </fieldset>
          <fieldset className="rack-audio-input-picker">
            <legend>Mono</legend>
            <div>
              {inputs.map((input) => {
                const active = sameChannels(channels, [input]);
                return (
                  <button
                    type="button"
                    key={input}
                    className={keyClass(active, [input])}
                    aria-pressed={active}
                    title={keyTitle([input])}
                    onClick={() => choose([input])}
                  >
                    {input}
                  </button>
                );
              })}
            </div>
          </fieldset>
          {pairs.length > 0 ? (
            <fieldset className="rack-audio-input-picker">
              <legend>Stereo</legend>
              <div>
                {pairs.map((pair) => {
                  const active = channels.length === 2
                    && sameChannels([...channels].sort((a, b) => a - b), pair);
                  const shown = active ? channels : pair;
                  return (
                    <button
                      type="button"
                      key={pair.join("-")}
                      className={keyClass(active, pair)}
                      aria-pressed={active}
                      title={keyTitle(pair)}
                      onClick={() => choose(active ? channels : pair)}
                    >
                      {shown.join("–")}
                    </button>
                  );
                })}
                <button
                  type="button"
                  className="rack-audio-swap"
                  disabled={channels.length !== 2}
                  onClick={() => choose([channels[1], channels[0]])}
                  title="Swap left and right"
                >
                  Swap L/R
                </button>
              </div>
            </fieldset>
          ) : null}
          {missing.length > 0 ? (
            <p className="rack-audio-link-status warning" role="alert">
              Input{missing.length > 1 ? "s" : ""} {formatInputList(missing)} {missing.length > 1 ? "are" : "is"} not
              captured by this host, so {missing.length > 1 ? "they are" : "it is"} silent. Capture{" "}
              {missing.length > 1 ? "them" : "it"} in the host's audio settings, or choose another.
            </p>
          ) : null}
        </section>

        <section className="rack-midi-section">
          <h3>Level</h3>
          <div className="rack-audio-level">
            <ScrubNumberField
              label="Trim"
              value={gainDb}
              minimum={AUDIO_INPUT_ROUTE_GAIN_MIN_DB}
              maximum={AUDIO_INPUT_ROUTE_GAIN_MAX_DB}
              suffix={`${gainDb > 0 ? "+" : ""}${gainDb} dB`}
              onChange={(gain_db) => setDraft((current) => ({ ...current, gain_db }))}
            />
            {knowsCapture && meterInputs.length > 0 ? (
              <AudioInputMeter
                feed={peaks}
                captured={status!.captured}
                inputs={meterInputs}
                gainDb={gainDb}
                className="rack-audio-link-meter"
                label="What this cable carries"
              />
            ) : null}
          </div>
        </section>
      </div>
      <footer>
        <button type="button" onClick={() => setDraft({ channels: [], gain_db: 0 })}>Reset</button>
        <span />
        <button type="button" onClick={onClose}>Cancel</button>
        <button
          type="button"
          className="primary"
          onClick={() => onApply(normalizeAudioInputRoute(draft))}
        >
          Apply
        </button>
      </footer>
    </section>
    </>
  );
}
