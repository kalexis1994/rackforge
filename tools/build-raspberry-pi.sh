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
install -d "$release/platforms/raspberry-pi" "$release/hardware"
for entry in appliance audio config etc provision sbin scripts systemd README.md
do
  cp -a "$repository/platforms/raspberry-pi/$entry" \
    "$release/platforms/raspberry-pi/$entry"
done
cp -a "$repository/hardware/controllers" "$release/hardware/controllers"

revision="${GITHUB_SHA:-$(git rev-parse HEAD)}"
printf 'revision=%s\narchitecture=linux-aarch64\nplugins=built-separately\n' \
  "$revision" >"$release/build-info.txt"

cat >"$release/INSTALL.md" <<'EOF'
# RackForge for Raspberry Pi ARM64

This artifact contains RackForge hosts and Raspberry Pi integration only.
Instrument plugins are versioned and distributed by their own pipelines.

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

Install and configure an instrument `.rfplugin` before creating `audio.toml`
or starting the audio service for unattended boot.
EOF

mkdir -p "$output_directory"
output_directory="$(cd "$output_directory" && pwd)"
epoch="${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}"
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
