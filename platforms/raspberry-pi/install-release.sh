#!/usr/bin/env bash
set -euo pipefail

repository="${RACKFORGE_REPOSITORY:-kalexis1994/rackforge}"
version="${RACKFORGE_VERSION:-latest}"
asset="RackForge-RaspberryPi-arm64.tar.gz"
checksums="SHA256SUMS.txt"

usage() {
  cat <<'EOF'
Install the latest RackForge host release on Raspberry Pi OS ARM64.

Usage:
  bash -o pipefail -c 'curl -fsSL https://raw.githubusercontent.com/kalexis1994/rackforge/main/platforms/raspberry-pi/install-release.sh | bash'

Optional environment variables:
  RACKFORGE_VERSION=v0.1.1       Install a specific release instead of latest.
  RACKFORGE_ROOT=/absolute/path  Override the default $HOME/rackforge root.
  RACKFORGE_OPTIMIZE=1           Apply reversible appliance optimizations.

The repository and its release must be public for unauthenticated downloads.
Instrument .rfplugin packages are installed separately after RackForge starts.
EOF
}

fail() {
  printf 'RackForge installer: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

download() {
  local url="$1"
  local destination="$2"
  curl \
    --proto '=https' \
    --tlsv1.2 \
    --fail \
    --location \
    --silent \
    --show-error \
    --retry 3 \
    --retry-all-errors \
    --connect-timeout 15 \
    --output "$destination" \
    "$url"
}

if [[ "${1:-}" == --help || "${1:-}" == -h ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 2
fi

for command in \
  awk bash curl cut date flock getent head hostname id install mktemp mv \
  realpath rm sha256sum sudo tar uname
do
  require_command "$command"
done

case "$(uname -m)" in
  aarch64|arm64) ;;
  *) fail "Raspberry Pi releases require a 64-bit ARM userspace" ;;
esac

[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
  || fail "invalid GitHub repository: $repository"
[[ "$version" == latest || "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-.][A-Za-z0-9.-]+)?$ ]] \
  || fail "invalid release version: $version"
[[ "${RACKFORGE_OPTIMIZE:-0}" =~ ^[01]$ ]] \
  || fail "RACKFORGE_OPTIMIZE must be 0 or 1"

requested_user="$(id -un)"
if [[ "$requested_user" == root ]]; then
  fail "run as the target user, not with sudo; the installer requests sudo when needed"
fi

passwd_entry="$(getent passwd "$requested_user" || true)"
[[ -n "$passwd_entry" ]] || fail "target user does not exist: $requested_user"
target_home="$(cut -d: -f6 <<<"$passwd_entry")"
root="${RACKFORGE_ROOT:-$target_home/rackforge}"
[[ "$root" == /* ]] || fail "RACKFORGE_ROOT must be an absolute path"
[[ ! "$root" =~ [[:space:]] ]] || fail "RACKFORGE_ROOT cannot contain whitespace"
target_home="$(realpath -m -- "$target_home")"
root="$(realpath -m -- "$root")"
case "$root" in
  /|/bin|/boot|/dev|/etc|/home|/lib|/lib64|/opt|/proc|/root|/run|/sbin|/srv|/sys|/tmp|/usr|/var|"$target_home")
    fail "RACKFORGE_ROOT is too broad: $root"
    ;;
esac

printf 'Checking administrator access for system services...\n'
sudo -v || fail "administrator access is required to install RackForge services"

install -d "$root"
exec 9>"$root/.install.lock"
flock -n 9 || fail "another RackForge installation is already running"

if [[ "$version" == latest ]]; then
  release_base="https://github.com/$repository/releases/latest/download"
else
  release_base="https://github.com/$repository/releases/download/$version"
fi

temporary="$(mktemp -d "${TMPDIR:-/tmp}/rackforge-install.XXXXXX")"
stage=""
backup=""
failed=""
cleanup() {
  [[ -z "$temporary" || ! -d "$temporary" ]] || rm -rf -- "$temporary"
  [[ -z "$stage" || ! -d "$stage" ]] || rm -rf -- "$stage"
}
trap cleanup EXIT

printf 'Downloading RackForge %s for Raspberry Pi ARM64...\n' "$version"
download "$release_base/$asset" "$temporary/$asset"
download "$release_base/$checksums" "$temporary/$checksums"

expected="$({ awk -v asset="$asset" '$2 == asset { print $1 }' "$temporary/$checksums"; } | head -n 1)"
[[ "$expected" =~ ^[0-9A-Fa-f]{64}$ ]] \
  || fail "$checksums does not contain a valid digest for $asset"
actual="$(sha256sum "$temporary/$asset" | awk '{ print $1 }')"
[[ "${actual,,}" == "${expected,,}" ]] \
  || fail "SHA-256 verification failed for $asset"
printf 'Verified %s\n' "$actual"

stage="$(mktemp -d "$root/.release-stage.XXXXXX")"
tar -xzf "$temporary/$asset" -C "$stage" --strip-components=1

for required in \
  target/release/rackforge-core \
  target/release/rackforge-web \
  target/release/rackforge-store \
  target/release/rackforge-platform-host \
  target/release/rackforge-controller-host \
  platforms/raspberry-pi/scripts/install.sh \
  platforms/raspberry-pi/scripts/install-appliance.sh \
  web/dist/index.html
do
  [[ -e "$stage/$required" ]] || fail "release is missing $required"
done

current="$root/current"
serial="$(date +%Y%m%d%H%M%S)-$$"
backup="$root/.current-backup-$serial"
failed="$root/.current-failed-$serial"
[[ ! -e "$backup" && ! -L "$backup" && ! -e "$failed" && ! -L "$failed" ]] \
  || fail "a release transaction already uses identifier $serial"
if [[ -e "$current" || -L "$current" ]]; then
  mv -- "$current" "$backup"
fi
mv -- "$stage" "$current"
stage=""

export RACKFORGE_USER="$requested_user"
export RACKFORGE_ROOT="$root"

restore_previous_release() {
  local reason="$1"
  local restore_error=""

  if [[ -e "$current" || -L "$current" ]]; then
    mv -- "$current" "$failed"
  fi
  if [[ -e "$backup" || -L "$backup" ]]; then
    mv -- "$backup" "$current"
    if ! bash "$current/platforms/raspberry-pi/scripts/install.sh"; then
      restore_error="; restoring the previous runtime also failed"
    elif ! bash "$current/platforms/raspberry-pi/scripts/install-appliance.sh"; then
      restore_error="; restoring the previous services also failed"
    fi
    fail "$reason; the previous release was restored$restore_error and the failed release remains at $failed"
  fi
  fail "$reason; this was the first installation and its files remain available for diagnosis at $failed"
}

if ! bash "$current/platforms/raspberry-pi/scripts/install.sh"; then
  restore_previous_release "runtime installation failed"
fi

appliance_arguments=()
if [[ "${RACKFORGE_OPTIMIZE:-0}" == 1 ]]; then
  appliance_arguments+=(--optimize)
fi
if ! bash "$current/platforms/raspberry-pi/scripts/install-appliance.sh" "${appliance_arguments[@]}"; then
  restore_previous_release "service installation failed"
fi

[[ ! -e "$backup" ]] || rm -rf -- "$backup"
address="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
printf '\nRackForge is installed at %s\n' "$root"
printf 'Open http://%s:8787 to select MIDI and audio devices.\n' \
  "${address:-RASPBERRY_PI_ADDRESS}"
