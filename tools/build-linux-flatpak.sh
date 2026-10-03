#!/usr/bin/env bash
# Builds RackForge Desktop for Linux as a Flatpak bundle, Standard edition:
# dist/linux-flatpak/RackForge-Linux-x86_64.flatpak, installable on any
# distribution with Flatpak:
#
#   flatpak install --user RackForge-Linux-x86_64.flatpak
#
# The Web interface and the bundled plugins are prepared out here, as the
# other desktop builds prepare them; the application itself is compiled in
# the GNOME SDK by flatpak-builder (platforms/linux-flatpak).
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_directory="${1:-$repository/dist/linux-flatpak}"
case "$output_directory" in
  /*) ;;
  *) output_directory="$repository/$output_directory" ;;
esac
app_id="io.github.kalexis1994.RackForge"
manifest="$repository/platforms/linux-flatpak/$app_id.yml"

if command -v flatpak-builder >/dev/null; then
  builder=(flatpak-builder)
elif flatpak info org.flatpak.Builder >/dev/null 2>&1; then
  builder=(flatpak run org.flatpak.Builder)
else
  printf 'flatpak-builder is required: install it, or run\n' >&2
  printf '  flatpak install flathub org.flatpak.Builder\n' >&2
  exit 2
fi
for command in cargo pnpm python3 git; do
  command -v "$command" >/dev/null
done

cd "$repository"
python3 tools/fetch-official-plugins.py \
  --output-directory dist/bundled-plugins/official
if [[ ! -f dist/bundled-plugins/RF-Concert-Grand.rfplugin ]]; then
  rustup target add wasm32-unknown-unknown
  cargo build --locked --release --target wasm32-unknown-unknown -p rackforge-concert-grand
  cargo run --locked --release -p rackforge-store -- pack-wasm \
    plugins/concert-grand/package \
    target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm \
    dist/bundled-plugins/RF-Concert-Grand.rfplugin
fi
pnpm --dir web install --frozen-lockfile
pnpm --dir web build

# The revision the About page and the health endpoint report. The build
# copies this over REVISION, as the sandbox carries no .git to ask.
install -d dist/linux-flatpak "$output_directory"
git rev-parse --short HEAD >dist/linux-flatpak/REVISION

state="$output_directory/.flatpak-builder"
build="$output_directory/build"
repo="$output_directory/repo"
"${builder[@]}" --user --install-deps-from=flathub --force-clean \
  --state-dir="$state" --repo="$repo" "$build" "$manifest"
flatpak build-bundle \
  --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  "$repo" "$output_directory/RackForge-Linux-x86_64.flatpak" "$app_id"

printf 'edition=standard\n' >"$output_directory/build-info.txt"
printf 'RACKFORGE_FLATPAK_READY %s\n' "$output_directory/RackForge-Linux-x86_64.flatpak"
