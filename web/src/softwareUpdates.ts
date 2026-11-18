export interface SoftwareUpdateState {
  current_version: string;
  build: string;
  platform: string;
  phase: "idle" | "checking" | "up_to_date" | "available" | "downloading" | "ready" | "failed" | "unsupported" | "not_published" | "installing" | "recovering" | "installed" | "rolled_back" | "recovery_failed";
  available: { version: string; notes: string; download_bytes: number } | null;
  message: string | null;
  downloaded_bytes: number;
  can_stage: boolean;
  can_install?: boolean;
}

export function updateBusy(state: SoftwareUpdateState) {
  return ["checking", "downloading", "installing", "recovering"].includes(state.phase);
}

export function updateDescription(state: SoftwareUpdateState): string {
  switch (state.phase) {
    case "checking": return "Checking signed releases…";
    case "up_to_date": return "RackForge is up to date.";
    case "available": return `RackForge ${state.available?.version ?? ""} is available. Its release signature has been verified.`;
    case "downloading": return "Downloading the signed update package…";
    case "ready": return "The update package has been downloaded and verified.";
    case "installing": return "Installing RackForge. Audio will stop briefly; reconnecting automatically…";
    case "recovering": return "The update could not start. Restoring the previous installation…";
    case "installed": return state.message ?? `RackForge ${state.current_version} was installed and checked.`;
    case "rolled_back": return state.message ?? `The previous RackForge ${state.current_version} was restored.`;
    case "recovery_failed": return state.message ?? "Recovery needs attention. Retry recovery or check the service logs.";
    case "failed": return state.message ?? "The update could not be verified. Try again.";
    case "not_published": return "No signed RackForge update has been published yet. You can check again later.";
    case "unsupported": return state.message ?? "Updates are unavailable on this host.";
    default: return "Check whether a newer signed RackForge release is available.";
  }
}
