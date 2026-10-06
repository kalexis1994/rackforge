#!/usr/bin/env bash
# Runs the resource engine against a scratch tree and a pretend systemctl,
# and checks every promise it makes: changes only what differs, records each
# original once, reads "applied" from the system, reverts exactly -- keeping
# what the user changed in the meantime -- and drops what a profile stops
# asking for.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
export DEDICATED_ROOT="$scratch/root"
export DEDICATED_SYSTEMCTL="$here/fake-systemctl"
export FAKE_UNITS="$scratch/units"
units="$FAKE_UNITS"
mkdir -p "$units" "$DEDICATED_ROOT/etc/systemd/system" "$DEDICATED_ROOT/boot/firmware"

failures=0
check() {
  local what="$1"
  shift
  if "$@"; then
    printf 'ok    %s\n' "$what"
  else
    printf 'FAIL  %s\n' "$what"
    failures=$((failures + 1))
  fi
}
same() { [[ "$1" == "$2" ]] || { printf '      expected %q\n      got      %q\n' "$2" "$1"; return 1; }; }

# The system as the image left it.
echo enabled >"$units/bluetooth.service"
echo enabled >"$units/apt-daily.timer"
printf 'Storage=auto\n' >"$DEDICATED_ROOT/etc/existing.conf"
printf 'dtparam=audio=on\n[pi5]\ndtoverlay=nospi10\n' >"$DEDICATED_ROOT/boot/firmware/config.txt"
printf 'console=tty1 root=PARTUUID=1 rootwait\n' >"$DEDICATED_ROOT/boot/firmware/cmdline.txt"
cp -a "$DEDICATED_ROOT/boot/firmware/config.txt" "$scratch/config.before"
cp -a "$DEDICATED_ROOT/boot/firmware/cmdline.txt" "$scratch/cmdline.before"

run() {
  # A fresh engine per run, as each invocation of rackforge-pi is.
  local profile="$1" verb="$2"
  (
    source "$here/../lib/engine.sh"
    CURRENT_MODULE=test
    want_file /etc/existing.conf 644 <<<'Storage=volatile'
    want_file /etc/systemd/system/rackforge-test.service 644 <<<'[Service]'
    want_unit bluetooth.service masked now
    want_line /boot/firmware/config.txt 'camera_auto_detect=0'
    want_token threadirqs present
    if [[ "$profile" == full ]]; then
      want_unit apt-daily.timer disabled
    fi
    case "$verb" in
      plan) engine_plan ;;
      apply) engine_apply ;;
      verify) engine_verify ;;
      revert) engine_revert ;;
    esac
  )
}

# 1. A first apply changes what differs and records originals.
out="$(run full apply)"
check "first apply applies all six" same "$(grep -c '^applied:' <<<"$out")" 6
check "a file is rewritten" same "$(cat "$DEDICATED_ROOT/etc/existing.conf")" "Storage=volatile"
check "a unit is masked" same "$(cat "$units/bluetooth.service")" masked
check "a timer is disabled" same "$(cat "$units/apt-daily.timer")" disabled
check "the line lands under a new [all]" same "$(tail -2 "$DEDICATED_ROOT/boot/firmware/config.txt")" $'[all]\ncamera_auto_detect=0'
check "cmdline.txt stays one line" same "$(wc -l <"$DEDICATED_ROOT/boot/firmware/cmdline.txt" | tr -d ' ')" 1
check "the token is added" grep -q ' threadirqs$' "$DEDICATED_ROOT/boot/firmware/cmdline.txt"
check "verify passes" run full verify

# 2. A second apply changes nothing.
out="$(run full apply)"
check "second apply applies nothing" same "$(grep -c '^applied:' <<<"$out" || true)" 0

# 3. Drift is seen by verify and repaired by apply; the original stays the
#    one from before RackForge.
printf 'Storage=persistent\n' >"$DEDICATED_ROOT/etc/existing.conf"
fails() { ! "$@" >/dev/null; }
check "verify sees drift" fails run full verify
run full apply >/dev/null
check "apply repairs drift" same "$(cat "$DEDICATED_ROOT/etc/existing.conf")" "Storage=volatile"

# 4. The user edits config.txt after RackForge did.
printf 'dtoverlay=hifiberry-dac\n' >>"$DEDICATED_ROOT/boot/firmware/config.txt"

# 5. A profile that stops asking for something gets it reverted.
out="$(run lean apply)"
check "a dropped wish is reverted" grep -q 'reverted: unit:apt-daily.timer' <<<"$out"
check "the timer is enabled again" same "$(cat "$units/apt-daily.timer")" enabled

# 6. An interrupted run is said and simply run again.
printf '%s begin unit:bluetooth.service\n' "$(date -u +%FT%TZ)" >>"$DEDICATED_ROOT/var/lib/rackforge/dedicated/journal"
out="$(run lean apply 2>&1)"
check "an interrupted step is reported" grep -q 'stopped part-way' <<<"$out"
out="$(run lean apply 2>&1)"
check "and only until a run completes" fails grep -q 'stopped part-way' <<<"$out"

# 7. Revert puts back exactly what was there, and keeps the user's edit.
run lean revert >/dev/null
check "the file is restored" same "$(cat "$DEDICATED_ROOT/etc/existing.conf")" "Storage=auto"
check "the created unit file is removed" test ! -e "$DEDICATED_ROOT/etc/systemd/system/rackforge-test.service"
check "the unit is enabled again" same "$(cat "$units/bluetooth.service")" enabled
check "config.txt keeps the user's line" grep -qx 'dtoverlay=hifiberry-dac' "$DEDICATED_ROOT/boot/firmware/config.txt"
check "config.txt loses RackForge's line" fails grep -qx 'camera_auto_detect=0' "$DEDICATED_ROOT/boot/firmware/config.txt"
check "cmdline.txt is as it was" cmp -s "$DEDICATED_ROOT/boot/firmware/cmdline.txt" "$scratch/cmdline.before"
check "no originals are left" test -z "$(ls -A "$DEDICATED_ROOT/var/lib/rackforge/dedicated/originals")"

if ((failures)); then
  printf '%d checks failed\n' "$failures"
  exit 1
fi
printf 'all checks passed\n'
