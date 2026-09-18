import { AsyncSpinner } from "../components/AsyncSpinner";

export function ConnectionBadge({ status }: { status: string }) {
  const label =
    status === "online"
      ? "System ready"
      : status === "idle"
        ? "No plugin installed"
        : status === "connecting"
          ? "Connecting"
          : "System offline";
  return (
    <div className={`connection-badge ${status}`}>
      {status === "connecting" ? (
        <AsyncSpinner label="Connecting to RackForge Core…" />
      ) : (
        <span className="status-dot" />
      )}
      <span>{label}</span>
    </div>
  );
}
