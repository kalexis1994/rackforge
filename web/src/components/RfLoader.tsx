import { useId } from "react";

export type RfLoaderSize = "compact" | "medium" | "large";

interface RfLoaderProps {
  label?: string;
  detail?: string;
  size?: RfLoaderSize;
  className?: string;
}

/**
 * Each limb of the mark is its own progress tube: it fills from the node it
 * hangs off, along the direction the artwork is drawn in.
 *
 *   bowl    from the input jack, rightwards around the D
 *   leg     from the F node, down and then away to the left
 *   arm     from the F node, rightwards to the output arrow
 *   spine   from the F node, upwards and then right
 *
 * The leg is the one path that had to be re-drawn: the mark traces it from the
 * R towards the node, and it has to fill the other way. Same geometry, walked
 * backwards, so the bowl still truncates it where the two cross.
 *
 * Each limb is stroked twice — an unlit copy underneath, a lit copy over it
 * revealed with `stroke-dashoffset`. `pathLength="1"` normalises every limb to
 * the same 0→1 scale, so they finish together without measuring anything.
 */
const LIMBS = [
  { key: "leg", d: "M550 200V297Q550 342 505 342H430L305 200" },
  { key: "spine", d: "M550 200V111Q550 68 595 68H720" },
  { key: "bowl", d: "M80 200H360Q410 200 410 150V110Q410 68 365 68H155V145" },
] as const;

/**
 * The middle arm is the one limb that does not use the dash reveal, because its
 * run does not end at the end of its stroke — it ends at the point of the
 * arrow, and an arrowhead is a fill, not something a stroke can carry.
 *
 * So arm and arrow are revealed together by one wipe. The arm is straight, so a
 * rectangle growing along it is exactly the dash reveal by another name, and
 * the arrow simply falls inside the same sweep. Lighting the arrow separately
 * is what made it arrive late and detached.
 */
const ARM_D = "M550 200H731";
const ARROW_D = "M720 174L762 200L720 226Z";
/** From the node at x=550 to the point of the arrow at x=762. */
const ARM_RUN = 212;

export function RfLoader({
  label = "RackForge",
  detail,
  size = "medium",
  className = "",
}: RfLoaderProps) {
  const uid = useId().replace(/:/g, "");
  const maskId = `rf-loader-${uid}`;
  const wipeId = `rf-loader-wipe-${uid}`;
  const classes = [
    "rf-async-loader",
    `rf-async-loader--${size}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={classes}
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <span className="rf-async-loader__mark" aria-hidden="true">
        <svg viewBox="0 0 704 308" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="800" height="400">
              <rect width="800" height="400" fill="#fff" />
              <circle cx="80" cy="200" r="10" fill="#000" />
              <circle cx="550" cy="200" r="12" fill="#000" />
            </mask>
            <mask id={wipeId} maskUnits="userSpaceOnUse" x="540" y="168" width={ARM_RUN + 12} height="64">
              <rect className="rf-arm-wipe" x="550" y="168" height="64" fill="#fff" />
            </mask>
          </defs>
          <g
            mask={`url(#${maskId})`}
            transform="translate(-58 -51)"
            fill="none"
            strokeWidth="34"
            strokeLinecap="butt"
            strokeLinejoin="round"
          >
            {LIMBS.map(({ key, d }) => (
              <g key={key} className={`rf-limb rf-limb--${key}`}>
                <path className="rf-limb__unlit" d={d} />
                <path className="rf-limb__lit" d={d} pathLength={1} />
              </g>
            ))}
            <g className="rf-limb rf-limb--arm">
              <path className="rf-limb__unlit" d={ARM_D} />
              <path className="rf-tip rf-tip--unlit" d={ARROW_D} strokeWidth="0" />
              {/* Two groups, not one: a filter is applied before masking, so a
                  glow declared on the masked group would be cut off against the
                  wipe rectangle. The outer group carries the glow, the inner
                  one the reveal. */}
              <g className="rf-arm__lit">
                <g mask={`url(#${wipeId})`}>
                  <path className="rf-limb__lit rf-limb__lit--undashed" d={ARM_D} />
                  <path className="rf-tip rf-tip--lit" d={ARROW_D} strokeWidth="0" />
                </g>
              </g>
            </g>
            {/* The nodes strike before anything runs: the input jack catches
                first, the patch point follows, and only then does the signal
                leave them. Each is two layers so a flicker dims to the unlit
                fitting rather than punching a hole through to the chassis. */}
            <g className="rf-node rf-node--jack">
              <circle className="rf-node__unlit" cx="80" cy="200" r="22" strokeWidth="0" />
              <circle className="rf-node__lit" cx="80" cy="200" r="22" strokeWidth="0" />
            </g>
            <g className="rf-node rf-node--patch">
              <circle className="rf-node__unlit" cx="550" cy="200" r="24" strokeWidth="0" />
              <circle className="rf-node__lit" cx="550" cy="200" r="24" strokeWidth="0" />
            </g>
          </g>
        </svg>
      </span>
      <span className="rf-async-loader__copy">
        <strong>{label}</strong>
        {detail && <small>{detail}</small>}
      </span>
    </div>
  );
}
