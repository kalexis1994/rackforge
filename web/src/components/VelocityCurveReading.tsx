import { useState } from "react";
import { VelocityCurveEditor } from "../components/VelocityCurveEditor";
import { type HostAudioPreferences } from "../types";
import { IDENTITY_VELOCITY_CURVE, type VelocityCurve } from "../velocityCurve";

/**
 * The velocity square, and which keybed it is reading for.
 *
 * A reading corrects a keyboard, so it belongs to the keyboard: a hammer
 * action spreads its velocities across the range and a pad grid piles them at
 * the top, and one reading for both is wrong for one of them. The first
 * option reads for every device that has none of its own, which is what a
 * player with one keyboard will use and never think about.
 */
export function VelocityCurveReading({
  draft,
  ports,
  sourceKeys,
  live,
  onChange,
}: {
  draft: HostAudioPreferences;
  ports: string[];
  sourceKeys: Record<string, number>;
  live: boolean;
  onChange: (draft: HostAudioPreferences) => void;
}) {
  const [port, setPort] = useState<string>("");
  const curves = draft.velocity_curves ?? {};
  const own = curves[port];
  const shown = port === "" ? (draft.velocity_curve ?? IDENTITY_VELOCITY_CURVE) : (own ?? draft.velocity_curve ?? IDENTITY_VELOCITY_CURVE);
  const ownSourceKeys = Object.keys(curves)
    .map((name) => sourceKeys[name])
    .filter((key): key is number => typeof key === "number");
  const write = (curve: VelocityCurve) => {
    if (port === "") {
      onChange({ ...draft, velocity_curve: curve });
      return;
    }
    onChange({ ...draft, velocity_curves: { ...curves, [port]: curve } });
  };
  const forget = () => {
    const next = { ...curves };
    delete next[port];
    onChange({ ...draft, velocity_curves: next });
  };
  return (
    <>
      <label className="velocity-curve-port">
        <span>Reading for</span>
        <select value={port} onChange={(event) => setPort(event.target.value)}>
          <option value="">Every other device</option>
          {ports.map((name) => (
            <option key={name} value={name}>
              {name}
              {curves[name] ? " ·" : ""}
            </option>
          ))}
        </select>
      </label>
      <p>
        {port === ""
          ? "How hard a key was struck, for any keyboard without a reading of its own."
          : own
            ? "This keyboard has its own reading."
            : "This keyboard follows the shared reading. Drag a point to give it one."}
      </p>
      <VelocityCurveEditor
        curve={shown}
        onChange={write}
        live={live}
        sourceKey={port === "" ? null : (sourceKeys[port] ?? -1)}
        ownSourceKeys={ownSourceKeys}
      />
      {port !== "" && own ? (
        <button type="button" className="velocity-curve-forget" onClick={forget}>
          Use the shared reading
        </button>
      ) : null}
    </>
  );
}
