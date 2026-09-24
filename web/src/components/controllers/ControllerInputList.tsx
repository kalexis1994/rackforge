import {
  type ControllerDevice,
  groupInputs,
  inputMessageLabel,
  mappingsForInput,
  standardMeaning,
} from "../../controllerMapping";

/**
 * The controller's inputs, in the package's groups. Each lights while its
 * control moves, and says whether it has a mapping -- and whether one applies
 * to the plugin playing.
 */
export function ControllerInputList({
  device,
  selectedId,
  lit,
  playingPluginId,
  onSelect,
}: {
  device: ControllerDevice;
  selectedId: string | null;
  lit: ReadonlySet<string>;
  playingPluginId?: string;
  onSelect: (inputId: string) => void;
}) {
  return (
    <div className="controller-input-list" role="listbox" aria-label={`${device.name} controls`}>
      {groupInputs(device.inputs).map((group) => (
        <section key={group.group} className="controller-input-group" aria-label={group.group}>
          <h3>{group.group}</h3>
          <ul>
            {group.inputs.map((input) => {
              const assignments = mappingsForInput(device.map, input);
              const playing = assignments.some((entry) => entry.plugin_id === playingPluginId);
              const standard = standardMeaning(input, device.roles, device.actions);
              const selected = input.id === selectedId;
              return (
                <li key={input.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    className={[
                      "controller-input",
                      selected ? "selected" : "",
                      lit.has(input.id) ? "lit" : "",
                      assignments.length > 0 ? "mapped" : "",
                      playing ? "playing" : "",
                    ].filter(Boolean).join(" ")}
                    onClick={() => onSelect(input.id)}
                  >
                    <i className="controller-input-lamp" aria-hidden="true" />
                    <span className="controller-input-copy">
                      <strong>{input.name}</strong>
                      <small>
                        {assignments.length > 0
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
                    {lit.has(input.id) ? <span className="visually-hidden">Moving</span> : null}
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
