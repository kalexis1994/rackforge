/**
 * RackForge's version is the workspace's, in the root Cargo.toml: the one a
 * release bumps (`chore: 0.1.x`), shared by every host binary. The web build
 * reads it from there, so the interface names the same version the release
 * does.
 */
export function workspaceVersion(cargoToml: string): string | null {
  const lines = cargoToml.split(/\r?\n/);
  let inWorkspacePackage = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith("[")) {
      inWorkspacePackage = trimmed === "[workspace.package]";
      continue;
    }
    if (!inWorkspacePackage) continue;
    const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
    if (match) return match[1];
  }
  return null;
}
