import { useState } from "react";
import { describeMode } from "../controllerMapping";
import {
  type ParameterAssociation,
  associationControl,
  associationLayer,
  associationSource,
} from "../parameterAssociations";
import type { PluginParameterDescriptor } from "../types";
import { AsyncActionLabel } from "./AsyncSpinner";
import { ModalDialog } from "./ModalDialog";

/**
 * Every MIDI association of one parameter, for a parameter with more than the
 * control's menu lists: each can be edited or removed, another added, or all
 * removed at once. The list scrolls inside the dialog, however long it grows.
 */
export function ParameterAssociationsDialog({
  parameterName,
  parameter,
  associations,
  onAdd,
  onEdit,
  onRemove,
  onClose,
}: {
  parameterName: string;
  parameter?: PluginParameterDescriptor;
  associations: ParameterAssociation[];
  onAdd: () => void;
  onEdit: (association: ParameterAssociation) => void;
  onRemove: (associations: ParameterAssociation[]) => Promise<void>;
  onClose: () => void;
}) {
  const [removing, setRemoving] = useState<Set<string>>(() => new Set());
  const [confirmAll, setConfirmAll] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A session link on the parameter wins over the controller maps.
  const sessionWins = associations.some((association) => association.kind === "session");

  const remove = (targets: ParameterAssociation[]) => {
    setError(null);
    setRemoving((current) => new Set([...current, ...targets.map((target) => target.key)]));
    void onRemove(targets)
      .catch((reason: unknown) =>
        setError(reason instanceof Error ? reason.message : "Could not remove the association."),
      )
      .finally(() => {
        setConfirmAll(false);
        setRemoving((current) => {
          const next = new Set(current);
          for (const target of targets) next.delete(target.key);
          return next;
        });
      });
  };
  const busy = removing.size > 0;

  return (
    <ModalDialog
      eyebrow="MIDI associations"
      title={parameterName}
      className="parameter-link-dialog parameter-associations-dialog"
      onClose={onClose}
      dismissible={!busy}
      actions={
        <>
          {associations.length > 1 ? (
            <button
              type="button"
              className={`secondary-button danger${confirmAll ? " confirming" : ""}`}
              disabled={busy}
              onClick={() => (confirmAll ? remove(associations) : setConfirmAll(true))}
            >
              {confirmAll ? `Remove all ${associations.length}?` : "Remove all"}
            </button>
          ) : null}
          <button type="button" className="secondary-button" disabled={busy} onClick={onAdd}>
            Add MIDI Link…
          </button>
          <button type="button" className="primary-button" disabled={busy} onClick={onClose}>
            Done
          </button>
        </>
      }
    >
      {error ? <p className="parameter-link-error" role="alert">{error}</p> : null}
      {associations.length === 0 ? (
        <p className="parameter-associations-empty">Nothing drives this control from MIDI any more.</p>
      ) : (
        <ul className="controller-assignment-list parameter-associations-list">
          {associations.map((association) => {
            const shadowed = sessionWins && association.kind === "controller";
            const mode = association.kind === "session" ? association.link.mode : association.mapping.mode;
            const pending = removing.has(association.key);
            return (
              <li key={association.key} className={`controller-assignment${shadowed ? " pending" : ""}`}>
                <div className="controller-assignment-copy">
                  <strong>
                    {associationSource(association)}
                    <span className="controller-assignment-live">
                      {association.kind === "session" ? "This session" : "Controller map"}
                    </span>
                    {associationLayer(association) === "fn" ? (
                      <span className="controller-assignment-fn">Fn</span>
                    ) : null}
                  </strong>
                  <span>{associationControl(association)}</span>
                  <small>
                    {shadowed
                      ? "This session's link drives the control instead while it is here."
                      : describeMode(mode, parameter)}
                  </small>
                </div>
                <div className="controller-assignment-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => onEdit(association)}
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="secondary-button danger"
                    disabled={busy}
                    onClick={() => remove([association])}
                  >
                    <AsyncActionLabel active={pending} activeLabel="Removing…">Remove</AsyncActionLabel>
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </ModalDialog>
  );
}
