export function validPluginProgramName(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    value.trim().length <= 64 &&
    !/[\p{Cc}\p{Cf}]/u.test(value.trim())
  );
}
