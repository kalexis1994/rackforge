import { Pencil } from "lucide-react";
import { useEffect, useRef } from "react";
import { type ControllerCheck, checkHint, valuesLabel } from "../../controllerCheck";
import {
  type ControllerDevice,
  groupInputs,
  inputMessageLabel,
  isModifierInput,
  mappingsForInput,
  standardMeaning,
} from "../../controllerMapping";

/**
 * The controller's inputs, in the package's groups. Moving a control lights
 * its LED and brings its row into view, so the player finds the one in their
 * hand; its Edit button opens what it does. Each row also says whether the
 * control has a mapping -- and whether one applies to the plugin playing.
 */
export function ControllerInputList({
  device,
  selectedId,
  lit,
  latestLitId,
  playingPluginId,
  editLabel = "Edit",
  check,
  onSelect,
}: {
  device: ControllerDevice;
  selectedId: string | null;
  lit: ReadonlySet<string>;
  /** The control that moved last: its row is scrolled into view. */
  latestLitId?: string | null;
  playingPluginId?: string;
  editLabel?: string;
  /** A hardware check under way: each row says what its control was heard sending. */
  check?: ControllerCheck;
  onSelect: (inputId: string) => void;
}) {
  const list = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!latestLitId || !list.current) return;
    const row = [...list.current.querySelectorAll<HTMLElement>("[data-input-id]")].find(
      (element) => element.dataset.inputId === latestLitId,
    );
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    row?.scrollIntoView({ block: "nearest", behavior: reduced ? "auto" : "smooth" });
  }, [latestLitId]);

  return (
    <div ref={list} className="controller-input-list" aria-label={`${device.name} controls`}>
      {groupInputs(device.inputs).map((group) => (
        <section key={group.group} className="controller-input-group" aria-label={group.group}>
          <h3>{group.group}</h3>
          <ul>
            {group.inputs.map((input) => {
              const assignments = mappingsForInput(device.map, input);
              const playing = assignments.some((entry) => entry.plugin_id === playingPluginId);
              const standard = standardMeaning(input, device.roles, device.actions);
              const selected = input.id === selectedId;
              const moving = lit.has(input.id);
              const fn = isModifierInput(device.map, input);
              const heard = check?.heard[input.id];
              const hint = heard ? checkHint(input, heard) : null;
              return (
                <li
                  key={input.id}
                  data-input-id={input.id}
                  aria-current={selected ? "true" : undefined}
                  className={[
                    "controller-input",
                    selected ? "selected" : "",
                    moving ? "lit" : "",
                    assignments.length > 0 ? "mapped" : "",
                    playing ? "playing" : "",
                    check ? (heard ? "checked" : "unchecked") : "",
                    hint ? "doubtful" : "",
                  ].filter(Boolean).join(" ")}
                >
                  <i className="controller-input-lamp" aria-hidden="true" />
                  <span className="controller-input-copy">
                    <strong>{input.name}</strong>
                    <small>
                      {check
                        ? heard
                          ? `${inputMessageLabel(input)} · ${valuesLabel(heard, typeof input.midi.note === "number")}${hint ? ` · ${hint}` : ""}`
                          : `Not heard yet · ${inputMessageLabel(input)}`
                        : fn
                        ? "Fn button"
                        : assignments.length > 0
                          ? assignments.map((entry) => entry.plugin_name).join(", ")
                          : standard ?? inputMessageLabel(input)}
                    </small>
                  </span>
                  {assignments.length > 0 ? (
                    <span
                      className="controller-input-mark"
                      title={playing ? "Mapped in the plugin playing" : "Mapped in another plugin"}
                    >
                      {assignments.length}
                    </span>
                  ) : null}
                  <button
                    type="button"
                    className="controller-input-edit"
                    aria-label={`${editLabel} ${input.name}`}
                    aria-pressed={selected}
                    onClick={() => onSelect(input.id)}
                  >
                    <Pencil size={14} aria-hidden="true" />
                    <span>{editLabel}</span>
                  </button>
                  {moving ? <span className="visually-hidden">Moving</span> : null}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
