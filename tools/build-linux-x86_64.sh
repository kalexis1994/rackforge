#!/usr/bin/env bash
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_directory="${1:-$repository/dist/linux-x86_64}"

case "$(uname -m)" in
  x86_64|amd64) ;;
  *)
    printf 'RackForge Linux x86-64 releases must be built natively on x86-64, got %s.\n' \
      "$(uname -m)" >&2
    exit 2
    ;;
esac

for command in cargo pnpm tar python3; do
  command -v "$command" >/dev/null
done
bash -n "$repository/platforms/linux-x86_64/install.sh"

official_plugins="$repository/dist/bundled-plugins/official"
python3 "$repository/tools/fetch-official-plugins.py" \
  --output-directory "$official_plugins"

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

stage="$(mktemp -d "${TMPDIR:-/tmp}/rackforge-linux-x86-64.XXXXXX")"
cleanup() {
  rm -rf -- "$stage"
}
trap cleanup EXIT

release="$stage/rackforge"
install -d \
  "$release/target/release" "$release/web" "$release/config" \
  "$release/controller-packages" "$release/bundled-plugins" "$release/platforms"

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

RACKFORGE_SOURCE="$repository" bash \
  "$repository/hardware/controllers/arturia-keylab-essential-mk3/build-package.sh" \
  "$release/controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"

cp -a "$repository/web/dist" "$release/web/dist"
cp -a "$repository/config/." "$release/config/"
cp -a "$repository/platforms/linux-x86_64" "$release/platforms/linux-x86_64"
cp "$repository/THIRD_PARTY_NOTICES.md" "$release/THIRD_PARTY_NOTICES.md"

default_plugin_archive="${RACKFORGE_BUNDLED_PLUGIN:-}"
if [[ -z "$default_plugin_archive" && -f "$repository/dist/bundled-plugins/RackForge-Concert-Grand.rfplugin" ]]; then
  default_plugin_archive="$repository/dist/bundled-plugins/RackForge-Concert-Grand.rfplugin"
fi
if [[ -n "$default_plugin_archive" ]]; then
  test -f "$default_plugin_archive"
  install -m 0644 "$default_plugin_archive" \
    "$release/bundled-plugins/RackForge-Concert-Grand.rfplugin"
fi
shopt -s nullglob
for archive in "$official_plugins"/*.rfplugin; do
  install -m 0644 "$archive" "$release/bundled-plugins/$(basename "$archive")"
done
shopt -u nullglob

revision="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || printf 'unknown')}"
printf 'revision=%s\narchitecture=linux-x86-64\n' "$revision" \
  >"$release/build-info.txt"

cat >"$release/INSTALL.md" <<'EOF'
# Install RackForge for Linux x86-64

```bash
tar -xzf RackForge-Linux-x86_64.tar.gz
cd rackforge
bash platforms/linux-x86_64/install.sh
```

Run the installer as the ordinary user who will own the RackForge runtime. It
requests administrator access only for groups and systemd services.
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
  -czf "$output_directory/RackForge-Linux-x86_64.tar.gz" \
  rackforge

printf 'RackForge Linux x86-64: %s\n' \
  "$output_directory/RackForge-Linux-x86_64.tar.gz"
