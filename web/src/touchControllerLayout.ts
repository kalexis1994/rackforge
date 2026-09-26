/** Pads are square; the grid fits whichever of width or height runs out first. */
export const PAD_ASPECT_RATIO = 1;

export interface PadGridInputs {
  docked: boolean;
  /** The roomy layout: a wide controller on a tall screen. */
  regular: boolean;
  controllerWidth: number;
  controllerHeight: number;
  dockHeight: number;
  rows: number;
  columns: number;
}

export interface PadGridLayout {
  gap: number;
  cellHeight: number;
  gridWidth: number;
  gridHeight: number;
  /** Class suffix for how much room each pad has for its label. */
  densityClass: string;
}

/**
 * The pad grid's size for a controller of a given size.
 *
 * Pure so it can run in two places: in render, and straight from a dock
 * resize gesture, which sizes the grid frame by frame without re-rendering
 * the controller (see TouchControllerPage's dock resize).
 */
export function padGridLayout(inputs: PadGridInputs): PadGridLayout {
  const { docked, regular, controllerWidth, controllerHeight, dockHeight, rows, columns } = inputs;
  const gap = regular ? 9 : 7;
  const areaWidth = Math.max(1, Math.min(920, controllerWidth - (regular ? 28 : 14)));
  const dockChromeHeight = docked ? regular ? 48 : 42 : 0;
  const sizingHeight = docked ? dockHeight - dockChromeHeight : controllerHeight;
  const areaHeight = Math.max(1, sizingHeight - (regular ? 28 : 14));
  const cellHeight = Math.max(1, Math.min(
    (areaHeight - gap * (rows - 1)) / rows,
    (areaWidth - gap * (columns - 1)) / columns / PAD_ASPECT_RATIO,
  ));
  const cellWidth = cellHeight * PAD_ASPECT_RATIO;
  return {
    gap,
    cellHeight,
    gridWidth: cellWidth * columns + gap * (columns - 1),
    gridHeight: cellHeight * rows + gap * (rows - 1),
    densityClass: cellHeight < 54
      ? " micro"
      : cellHeight < 92
        ? " compact"
        : cellHeight < 132
          ? " condensed"
          : "",
  };
}
