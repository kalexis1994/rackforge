import { ModalDialog } from "../components/ModalDialog";

export function PlayModeTransitionDialog({
  onCancel,
  onConfirm,
}: {
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <ModalDialog
      eyebrow="LIVE output active"
      title="Switch to PLAY?"
      role="alertdialog"
      onClose={onCancel}
      closeLabel="Stay in LIVE"
      backdropClassName="play-mode-transition-backdrop"
      className="play-mode-transition-dialog"
      message={
        <div className="play-mode-transition-copy">
          <p>The current LIVE Rack will stop sounding and RackForge will activate the selected PLAY instrument.</p>
          <p>Continue only if you intend to leave the live performance.</p>
        </div>
      }
      actions={
        <>
          <button className="secondary-button" onClick={onCancel}>Stay in LIVE</button>
          <button className="primary-button" onClick={onConfirm}>Switch to PLAY</button>
        </>
      }
    />
  );
}
