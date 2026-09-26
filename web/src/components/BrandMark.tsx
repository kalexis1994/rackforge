import { useId } from "react";

// Inline rather than an <img> so the mark reads the lighting tokens: an image
// is its own document and cannot see the faceplate's palette.
//
// The paint order is the drawing. The leg is stroked first and the bowl of the
// R over it, so the bar truncates the leg at its lower edge instead of letting
// the diagonal tip run into the red; the node at the crossbar goes last because
// it covers the seam where three strokes meet. Reordering these is a visual
// change, not a refactor.
export function BrandMark() {
  // Two node holes are punched through the strokes underneath, so the knockout
  // has to be a mask. Ids must not collide between the rail and the about card.
  const maskId = `rf-mark-${useId().replace(/:/g, "")}`;
  return (
    <span className="brand-mark" aria-hidden="true">
      <svg viewBox="0 0 704 308" xmlns="http://www.w3.org/2000/svg">
        <defs>
          <mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="800" height="400">
            <rect width="800" height="400" fill="#fff" />
            <circle cx="80" cy="200" r="10" fill="#000" />
            <circle cx="550" cy="200" r="12" fill="#000" />
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
          <path d="M305 200L430 342H505Q550 342 550 297V200" stroke="var(--mark-leg)" />
          <path d="M550 200V111Q550 68 595 68H720" stroke="var(--mark-arm)" />
          <path d="M550 200H731" stroke="var(--mark-arm)" />
          <path
            d="M80 200H360Q410 200 410 150V110Q410 68 365 68H155V145"
            stroke="var(--mark-bowl)"
          />
          <circle cx="80" cy="200" r="22" fill="var(--mark-bowl)" stroke="none" />
          <circle cx="550" cy="200" r="24" fill="var(--mark-arm)" stroke="none" />
          <path d="M720 174L762 200L720 226Z" fill="var(--mark-arm)" stroke="none" />
        </g>
      </svg>
    </span>
  );
}
