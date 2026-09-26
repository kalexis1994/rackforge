import { useAudioInputPeaks, type AudioInputPeakFeed } from "../hooks/useAudioInputStatus";
import { meterLevel } from "../audioInputRoute";

/** A peak this close to full scale lights the clip mark. */
const CLIP_PEAK = 0.99;
/** Past this many inputs the bars stand side by side, a bank of them, so
 *  an eighteen-input interface is a strip and not a column. */
const BANK_ABOVE = 4;

/**
 * Bars for captured inputs, as they arrive. `inputs` picks which physical
 * inputs to show, in order -- a cable's own -- out of `captured`, the order
 * the peaks come in; an input that is not captured shows an empty, struck
 * bar. `gainDb` is a cable's trim, so the bar shows what that cable carries.
 */
export function AudioInputMeter({
  feed,
  captured,
  inputs = captured,
  gainDb = 0,
  className = "",
  label = "Input level",
}: {
  feed: AudioInputPeakFeed | null;
  captured: number[];
  inputs?: number[];
  gainDb?: number;
  className?: string;
  label?: string;
}) {
  const peaks = useAudioInputPeaks(feed);
  const gain = 10 ** (gainDb / 20);
  const bank = inputs.length > BANK_ABOVE;
  return (
    <div
      className={`audio-input-meter${bank ? " is-bank" : ""} ${className}`.trim()}
      role="group"
      aria-label={label}
    >
      {inputs.map((input) => {
        const index = captured.indexOf(input);
        const peak = index >= 0 ? (peaks[index] ?? 0) * gain : 0;
        const level = meterLevel(peak);
        return (
          <span
            key={input}
            className={`audio-input-meter-bar${index < 0 ? " is-missing" : ""}${peak >= CLIP_PEAK ? " is-clipping" : ""}`}
            title={index < 0 ? `Input ${input} is not captured` : `Input ${input}`}
          >
            <small>{input}</small>
            {/* The scale is drawn on the track, green to red; the cover
                over its unreached part is what moves, so a colour stays
                where its level is. */}
            <span aria-hidden="true">
              <i style={bank ? { height: `${(1 - level) * 100}%` } : { width: `${(1 - level) * 100}%` }} />
            </span>
          </span>
        );
      })}
    </div>
  );
}
