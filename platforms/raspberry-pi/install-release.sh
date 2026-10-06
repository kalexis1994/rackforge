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
  RACKFORGE_DEDICATED=1          Make the Pi a dedicated instrument: services,
                                 CPU, memory and storage set for RackForge,
                                 every change recorded and revertible with
                                 platforms/raspberry-pi/dedicated/rackforge-pi.
                                 A Pi made dedicated before is kept so, with
                                 the new release's profile, on every update.
  RACKFORGE_ROLLBACK=1           Return to the release installed before the
                                 current one, which every successful install
                                 keeps as previous/. A release is the programs,
                                 the interface and the bundled plugins: the
                                 configuration, the session, the plugin store
                                 and the system's settings are not part of it
                                 and stay as they are.

The repository and its release must be public for unauthenticated downloads.
RackForge includes its pinned official instruments. Additional .rfplugin
packages can be installed after RackForge starts.
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
[[ "${RACKFORGE_DEDICATED:-0}" =~ ^[01]$ ]] \
  || fail "RACKFORGE_DEDICATED must be 0 or 1"
[[ "${RACKFORGE_OPTIMIZE:-0}" == 0 || "${RACKFORGE_DEDICATED:-0}" == 0 ]] \
  || fail "RACKFORGE_DEDICATED supersedes RACKFORGE_OPTIMIZE; set one of them"
[[ "${RACKFORGE_ROLLBACK:-0}" =~ ^[01]$ ]] \
  || fail "RACKFORGE_ROLLBACK must be 0 or 1"
rollback="${RACKFORGE_ROLLBACK:-0}"

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
# sudo -v revalidates the user and asks for a password even under a
# NOPASSWD policy, which made the documented curl | bash one-liner
# impossible to run without a terminal. Asking whether a privileged
# command can run is the question that actually matters here.
sudo -n true 2>/dev/null || sudo -v || fail "administrator access is required to install RackForge services"

install -d "$root"
exec 9>"$root/.install.lock"
flock -n 9 || fail "another RackForge installation is already running"

current="$root/current"
previous="$root/previous"

# An installation killed part-way leaves its pieces behind; with the lock
# held, nothing else is using them. Killed between moving the old release
# aside and putting the new one in place, it left no current/ at all: the
# old release goes back first. Killed later, the old release is the one to
# keep as previous/.
mapfile -t stale_backups < <(ls -1d "$root"/.current-backup-* 2>/dev/null | sort)
if ((${#stale_backups[@]})); then
  newest_backup="${stale_backups[-1]}"
  if [[ ! -e "$current" && ! -L "$current" ]]; then
    mv -- "$newest_backup" "$current"
    printf 'Recovered the release an interrupted installation had moved aside.\n'
  else
    rm -rf -- "$previous"
    mv -- "$newest_backup" "$previous"
    printf 'Kept the release an interrupted installation had moved aside as previous/.\n'
  fi
  for backup in "${stale_backups[@]:0:${#stale_backups[@]}-1}"; do
    rm -rf -- "$backup"
  done
fi
# A failed release is kept for diagnosis; the newest one is enough.
mapfile -t failed_releases < <(ls -1d "$root"/.current-failed-* 2>/dev/null | sort)
if ((${#failed_releases[@]} > 1)); then
  for failed_release in "${failed_releases[@]:0:${#failed_releases[@]}-1}"; do
    rm -rf -- "$failed_release"
  done
fi

if [[ "$rollback" == 1 ]]; then
  [[ -d "$previous" ]] || fail "there is no previous release to return to"
fi

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

if [[ "$rollback" == 1 ]]; then
  printf 'Returning to the previous release...\n'
  incoming="$previous"
else
  printf 'Downloading RackForge %s for Raspberry Pi ARM64...\n' "$version"
  download "$release_base/$asset" "$temporary/$asset"
  download "$release_base/$checksums" "$temporary/$checksums"

  # sha256sum writes "<digest> *<name>" in binary mode and "<digest>  <name>"
  # in text mode. Matching the name exactly saw the asterisk as part of it and
  # found no digest at all, so every release published from a binary-mode run
  # failed verification before a single byte was installed.
  expected="$({ awk -v asset="$asset" 'substr($2, 1, 1) == "*" ? substr($2, 2) == asset : $2 == asset { print $1 }' "$temporary/$checksums"; } | head -n 1)"
  [[ "$expected" =~ ^[0-9A-Fa-f]{64}$ ]] \
    || fail "$checksums does not contain a valid digest for $asset"
  actual="$(sha256sum "$temporary/$asset" | awk '{ print $1 }')"
  [[ "${actual,,}" == "${expected,,}" ]] \
    || fail "SHA-256 verification failed for $asset"
  printf 'Verified %s\n' "$actual"

  stage="$(mktemp -d "$root/.release-stage.XXXXXX")"
  tar -xzf "$temporary/$asset" -C "$stage" --strip-components=1
  incoming="$stage"
fi

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
  [[ -e "$incoming/$required" ]] || fail "release is missing $required"
done

serial="$(date +%Y%m%d%H%M%S)-$$"
backup="$root/.current-backup-$serial"
failed="$root/.current-failed-$serial"
[[ ! -e "$backup" && ! -L "$backup" && ! -e "$failed" && ! -L "$failed" ]] \
  || fail "a release transaction already uses identifier $serial"
if [[ -e "$current" || -L "$current" ]]; then
  mv -- "$current" "$backup"
fi
mv -- "$incoming" "$current"
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

# The release this one replaced stays, as previous/, for RACKFORGE_ROLLBACK.
# A rollback's replaced release becomes previous/ in turn, so a rollback can
# itself be undone the same way.
if [[ -e "$backup" ]]; then
  rm -rf -- "$previous"
  mv -- "$backup" "$previous"
fi

# Dedicated mode changes the system, not the release: the release above is
# installed and running whatever happens here, so a failure is reported and
# left for `rackforge-pi`, never undone by restoring the previous release.
# A Pi that was made dedicated before keeps the profile this release ships.
dedicated="$current/platforms/raspberry-pi/dedicated/rackforge-pi"
if [[ "${RACKFORGE_DEDICATED:-0}" == 1 ]] \
  || sudo test -n "$(sudo ls -A /var/lib/rackforge/dedicated/originals 2>/dev/null)"; then
  if ! sudo --preserve-env=RACKFORGE_USER,RACKFORGE_ROOT "$dedicated" apply; then
    printf '\nRackForge is installed, but the dedicated profile did not fully apply.\n' >&2
    printf 'See: sudo %s plan\n' "$dedicated" >&2
  fi
fi

address="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
printf '\nRackForge is installed at %s\n' "$root"
# This host does not configure audio devices over the network -- that screen
# belongs to the desktop shell -- so pointing at it was an instruction nobody
# could follow. Activating an instrument is what writes config/audio.toml
# here, and the engine starts from that file.
printf 'Open http://%s:8787 and activate an instrument to start the engine.\n' \
  "${address:-RASPBERRY_PI_ADDRESS}"
printf 'Audio devices are chosen in %s/config/audio.toml.\n' \
  "$root"
printf 'The installed %s/config/audio.toml.example is a working start.\n' \
  "$root"
