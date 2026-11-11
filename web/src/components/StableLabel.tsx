/**
 * A word that changes in place -- "Saved" to "Unsaved changes" -- without
 * moving what is beside it. Every word it can show is laid in the same cell
 * and only the current one is visible, so the cell is always as wide as the
 * longest of them and nothing around it shifts when the word changes.
 */
export function StableLabel<T extends string>({
  value,
  options,
}: {
  value: T;
  options: readonly T[];
}) {
  return (
    <span className="stable-label">
      {options.map((option) => (
        <span key={option} aria-hidden={option === value ? undefined : true}>
          {option}
        </span>
      ))}
    </span>
  );
}
