interface ToggleSwitchProps {
  checked: boolean;
  label: string;
  description?: string;
  checkedLabel?: string;
  uncheckedLabel?: string;
  disabled?: boolean;
  className?: string;
  onChange: (checked: boolean) => void;
}

export function ToggleSwitch({
  checked,
  label,
  description,
  checkedLabel = "On",
  uncheckedLabel = "Off",
  disabled = false,
  className = "",
  onChange,
}: ToggleSwitchProps) {
  const stateLabel = checked ? checkedLabel : uncheckedLabel;
  return (
    <button
      type="button"
      className={`rf-switch${className ? ` ${className}` : ""}`}
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="rf-switch-copy">
        <strong>{label}</strong>
        {description ? <small>{description}</small> : null}
      </span>
      <span className="rf-switch-state" aria-hidden="true">
        <span>{stateLabel}</span>
        <i><b /></i>
      </span>
    </button>
  );
}
