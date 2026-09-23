import type { RackGraphPosition } from "../types";

/** Space kept between the editor and the edges of the graph it floats in. */
export const POPOVER_MARGIN = 12;
/** Where the tail sits below the editor's top edge, pointing at the node. */
export const POPOVER_TAIL_TOP = 28;

/**
 * Where the editor is drawn and how big, inside the graph's visible area.
 *
 * It opens beside its node at a fixed size, and nothing kept it inside the
 * graph: a node in the lower half put the bottom of the editor -- the end of
 * the instrument's own interface, and the footer -- past the edge, where the
 * layer clips it. The size the player chose is kept as a preference; what is
 * drawn is that size where it fits and the room there is where it does not,
 * moved in from any edge it would cross.
 */
export function fitPopover(
  position: RackGraphPosition,
  size: { width: number; height: number },
  bounds: { width: number; height: number } | null,
) {
  if (!bounds) return { position, size };
  const width = Math.max(0, Math.min(size.width, bounds.width - POPOVER_MARGIN * 2));
  const height = Math.max(0, Math.min(size.height, bounds.height - POPOVER_MARGIN * 2));
  const clamp = (value: number, low: number, high: number) =>
    Math.min(Math.max(value, low), Math.max(low, high));
  return {
    position: {
      x: clamp(position.x, POPOVER_MARGIN, bounds.width - width - POPOVER_MARGIN),
      y: clamp(position.y, POPOVER_MARGIN, bounds.height - height - POPOVER_MARGIN),
    },
    size: { width, height },
  };
}
