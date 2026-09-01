#!/usr/bin/env bash
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Resolved before anything changes directory, so a relative argument always
# means the same place as the default rather than wherever the build happens
# to be standing when it creates it.
output_directory="${1:-$repository/dist/raspberry-pi}"
case "$output_directory" in
  /*) ;;
  *) output_directory="$repository/$output_directory" ;;
esac
architecture="$(uname -m)"
edition="${RACKFORGE_EDITION:-standard}"

case "$edition" in
  standard|minimal) ;;
  *)
    printf 'RACKFORGE_EDITION must be standard or minimal, got %s.\n' \
      "$edition" >&2
    exit 2
    ;;
esac

case "$architecture" in
  aarch64|arm64) ;;
  *)
    printf 'RackForge Raspberry Pi releases must be built natively on ARM64, got %s.\n' \
      "$architecture" >&2
    exit 2
    ;;
esac

command -v cargo >/dev/null
command -v tar >/dev/null
command -v python3 >/dev/null
bash -n "$repository/platforms/raspberry-pi/install-release.sh"

official_plugins="$repository/dist/bundled-plugins/official"
if [[ "$edition" == standard ]]; then
  official_plugin_override="${RACKFORGE_OFFICIAL_PLUGIN:-}"
  if [[ -n "$official_plugin_override" ]]; then
    [[ "$official_plugin_override" == /* ]] || {
      printf 'RACKFORGE_OFFICIAL_PLUGIN must be an absolute path: %s\n' \
        "$official_plugin_override" >&2
      exit 2
    }
    [[ -f "$official_plugin_override" ]] || {
      printf 'RACKFORGE_OFFICIAL_PLUGIN is not a file: %s\n' \
        "$official_plugin_override" >&2
      exit 2
    }
    install -d "$official_plugins"
    install -m 0644 "$official_plugin_override" \
      "$official_plugins/$(basename "$official_plugin_override")"
  else
    python3 "$repository/tools/fetch-official-plugins.py" \
      --output-directory "$official_plugins"
  fi
fi

web_dist="${RACKFORGE_WEB_DIST:-}"
if [[ -z "$web_dist" ]]; then
  command -v node >/dev/null
  command -v pnpm >/dev/null
  cd "$repository"
  pnpm --dir web install --frozen-lockfile
  pnpm --dir web build
  web_dist="$repository/web/dist"
else
  [[ "$web_dist" == /* ]] || {
    printf 'RACKFORGE_WEB_DIST must be an absolute path: %s\n' "$web_dist" >&2
    exit 2
  }
  web_dist="$(cd "$web_dist" && pwd)"
fi
[[ -f "$web_dist/index.html" ]] || {
  printf 'RackForge Web build is missing index.html: %s\n' "$web_dist" >&2
  exit 2
}

cd "$repository"

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

cp -a "$web_dist" "$release/web/dist"
cp -a "$repository/config/." "$release/config/"
default_plugin_archive="${RACKFORGE_BUNDLED_PLUGIN:-}"
install -d "$release/bundled-plugins"
if [[ "$edition" == standard ]]; then
  if [[ -z "$default_plugin_archive" && -f "$repository/dist/bundled-plugins/RF-Concert-Grand.rfplugin" ]]; then
    default_plugin_archive="$repository/dist/bundled-plugins/RF-Concert-Grand.rfplugin"
  fi
  if [[ -n "$default_plugin_archive" ]]; then
    [[ -f "$default_plugin_archive" ]] || {
      printf 'RACKFORGE_BUNDLED_PLUGIN is not a file: %s\n' \
        "$default_plugin_archive" >&2
      exit 2
    }
    install -m 0644 "$default_plugin_archive" \
      "$release/bundled-plugins/RF-Concert-Grand.rfplugin"
  fi
  shopt -s nullglob
  for archive in "$official_plugins"/*.rfplugin; do
    install -m 0644 "$archive" \
      "$release/bundled-plugins/$(basename "$archive")"
  done
  shopt -u nullglob
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
if [[ -f "$release/bundled-plugins/RF-Concert-Grand.rfplugin" ]]; then
  default_plugin=org.rackforge.concert-grand
fi
official_plugin="$(python3 - "$release/bundled-plugins" <<'PY'
import pathlib
import re
import sys
import zipfile

directory = pathlib.Path(sys.argv[1])
carried = []
for archive_path in sorted(directory.glob("*.rfplugin")):
    if archive_path.name == "RF-Concert-Grand.rfplugin":
        continue
    with zipfile.ZipFile(archive_path) as archive:
        manifest = archive.read("rackforge-plugin.toml").decode("utf-8")
    plugin_id = re.search(r'^id = "([^"]+)"$', manifest, re.MULTILINE)
    version = re.search(r'^version = "([^"]+)"$', manifest, re.MULTILINE)
    if plugin_id is None or version is None:
        raise SystemExit(f"{archive_path.name} has invalid identity metadata")
    carried.append(f"{plugin_id.group(1)}@{version.group(1)}")
print(",".join(carried) if carried else "none")
PY
)"
printf 'revision=%s\narchitecture=linux-aarch64\nedition=%s\ndefault_plugin=%s\nofficial_plugin=%s\n' \
  "$revision" "$edition" "$default_plugin" "$official_plugin" >"$release/build-info.txt"

cat >"$release/INSTALL.md" <<'EOF'
# RackForge for Raspberry Pi ARM64

This artifact contains the RackForge hosts and Raspberry Pi integration.
The Standard edition also bundles RackForge Concert Grand and the pinned
official instrument packages listed in build-info.txt. The Minimal edition
carries no instruments at all; install the ones you want from Plugin
Manager. Instruments such as RF-Soundfonts are versioned and distributed
by their own pipelines.

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

printf 'RackForge Raspberry Pi (%s): %s\n' \
  "$edition" "$output_directory/RackForge-RaspberryPi-arm64.tar.gz"
