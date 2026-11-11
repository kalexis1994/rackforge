import { Piano } from "lucide-react";

/**
 * Slides the on-screen keyboard out over the current surface.
 *
 * Deliberately quieter than a nav key: it is a latching switch on the chassis,
 * not a place you go, and it sits with the lighting selector rather than among
 * the destinations it opens on top of.
 */
export function ControllerDockToggle({
  open,
  onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className={`controller-dock-toggle${open ? " open" : ""}`}
      aria-pressed={open}
      onClick={onToggle}
    >
      <Piano aria-hidden="true" strokeWidth={1.9} />
      <span>Touch Controller</span>
    </button>
  );
}
