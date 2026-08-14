#!/usr/bin/env bash
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_directory="${1:-$repository/dist/raspberry-pi}"
architecture="$(uname -m)"

case "$architecture" in
  aarch64|arm64) ;;
  *)
    printf 'RackForge Raspberry Pi releases must be built natively on ARM64, got %s.\n' \
      "$architecture" >&2
    exit 2
    ;;
esac

command -v cargo >/dev/null
command -v pnpm >/dev/null
command -v tar >/dev/null
bash -n "$repository/platforms/raspberry-pi/install-release.sh"

cd "$repository"
pnpm --dir web install --frozen-lockfile
pnpm --dir web build

cargo build --locked --release \
  -p rackforge-core \
  -p rackforge-web \
  -p rackforge-store \
  -p rackforge-platform-host \
  -p rackforge-controller-host \
  -p rackforge-controller-arturia-keylab-essential-mk3

stage="$(mktemp -d "${TMPDIR:-/tmp}/rackforge-pi-release.XXXXXX")"
cleanup() {
  rm -rf -- "$stage"
}
trap cleanup EXIT

release="$stage/rackforge"
install -d \
  "$release/target/release" \
  "$release/web" \
  "$release/config"

for binary in \
  rackforge-core \
  rackforge-web \
  rackforge-store \
  rackforge-platform-host \
  rackforge-controller-host \
  rackforge-arturia-keylab-essential-mk3-driver
do
  install -m 0755 "$repository/target/release/$binary" \
    "$release/target/release/$binary"
done

install -d "$release/controller-packages"
RACKFORGE_SOURCE="$repository" bash \
  "$repository/hardware/controllers/arturia-keylab-essential-mk3/build-package.sh" \
  "$release/controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"

cp -a "$repository/web/dist" "$release/web/dist"
cp -a "$repository/config/." "$release/config/"
if [[ -n "${RACKFORGE_BUNDLED_PLUGIN:-}" ]]; then
  [[ -f "$RACKFORGE_BUNDLED_PLUGIN" ]] || {
    printf 'RACKFORGE_BUNDLED_PLUGIN is not a file: %s\n' \
      "$RACKFORGE_BUNDLED_PLUGIN" >&2
    exit 2
  }
  install -d "$release/bundled-plugins"
  install -m 0644 "$RACKFORGE_BUNDLED_PLUGIN" \
    "$release/bundled-plugins/RF-Soundfonts.rfplugin"
fi
install -d "$release/platforms/raspberry-pi" "$release/hardware"
for entry in appliance audio config etc provision sbin scripts systemd README.md install-release.sh
do
  cp -a "$repository/platforms/raspberry-pi/$entry" \
    "$release/platforms/raspberry-pi/$entry"
done
cp -a "$repository/hardware/controllers" "$release/hardware/controllers"
cp "$repository/THIRD_PARTY_NOTICES.md" "$release/THIRD_PARTY_NOTICES.md"

revision="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || printf 'unknown')}"
default_plugin=none
if [[ -f "$release/bundled-plugins/RF-Soundfonts.rfplugin" ]]; then
  default_plugin=org.rackforge.rf-soundfonts
fi
printf 'revision=%s\narchitecture=linux-aarch64\ndefault_plugin=%s\n' \
  "$revision" "$default_plugin" >"$release/build-info.txt"

cat >"$release/INSTALL.md" <<'EOF'
# RackForge for Raspberry Pi ARM64

This artifact contains the RackForge hosts, Raspberry Pi integration, and the
verified RF-Soundfonts default instrument. Other instruments are versioned and
distributed by their own pipelines.

Extract it for the user who will run RackForge. The installer derives the
deployment root from that user's home directory; it can also be overridden
with RACKFORGE_USER and RACKFORGE_ROOT.

```bash
mkdir -p "$HOME/rackforge/current"
tar -xzf RackForge-RaspberryPi-arm64.tar.gz \
  -C "$HOME/rackforge/current" --strip-components=1
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install.sh"
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh"
```

Select the MIDI and audio devices before creating `audio.toml` or starting the
audio service for unattended boot.
EOF

mkdir -p "$output_directory"
output_directory="$(cd "$output_directory" && pwd)"
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
  epoch="$SOURCE_DATE_EPOCH"
elif epoch="$(git show -s --format=%ct HEAD 2>/dev/null)" && [[ -n "$epoch" ]]; then
  :
else
  epoch="$(date +%s)"
fi
tar \
  --sort=name \
  --mtime="@$epoch" \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C "$stage" \
  -czf "$output_directory/RackForge-RaspberryPi-arm64.tar.gz" \
  rackforge

printf 'RackForge Raspberry Pi: %s\n' \
  "$output_directory/RackForge-RaspberryPi-arm64.tar.gz"
