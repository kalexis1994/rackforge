#!/usr/bin/env bash
# Pins every CPU to the performance governor before the audio path starts.
#
# On a Pi the default `ondemand` governor reacts to load *after* it appears.
# An audio period is 2.7 ms at 128 frames, so the governor's own reaction
# latency is longer than the deadline it is supposed to protect: the first
# dense chord after an idle stretch is exactly when the clock is still low.
#
# Written straight to sysfs rather than through `cpufreq-set` so the appliance
# does not depend on the cpufrequtils package being installed and current.
# Unlike a one-way tuning script, this records the previous governor so
# `restore` can put the machine back the way it was found.
set -euo pipefail

action="${1:-status}"
state_root="${RACKFORGE_APPLIANCE_STATE:-/var/lib/rackforge/appliance}"
state_file="$state_root/cpu-governor.saved"
cpu_root=/sys/devices/system/cpu

usage() {
  printf 'usage: %s apply|restore|status\n' "$0" >&2
  exit 2
}

governor_paths() {
  # Absent on virtualised or cpufreq-less kernels; an empty list is not a fault.
  compgen -G "$cpu_root/cpu[0-9]*/cpufreq/scaling_governor" || true
}

available_governors() {
  local path="$cpu_root/cpu0/cpufreq/scaling_available_governors"
  [[ -r "$path" ]] && cat "$path" || true
}

status() {
  local paths current
  paths="$(governor_paths)"
  if [[ -z "$paths" ]]; then
    printf 'CPU_GOVERNOR state=unavailable\n'
    return 0
  fi
  current="$(cat "$cpu_root/cpu0/cpufreq/scaling_governor" 2>/dev/null || printf unknown)"
  printf 'CPU_GOVERNOR state=%s saved=%s\n' \
    "$current" \
    "$([[ -f "$state_file" ]] && cat "$state_file" || printf none)"
}

apply() {
  local paths path
  paths="$(governor_paths)"
  if [[ -z "$paths" ]]; then
    # Not a failure: a host without cpufreq has nothing to pin, and refusing to
    # boot the audio path over a missing tuning knob is the worse outcome.
    printf 'CPU_GOVERNOR_SKIPPED reason=cpufreq_unavailable\n'
    return 0
  fi
  if ! grep -qw performance <<<"$(available_governors)"; then
    printf 'CPU_GOVERNOR_SKIPPED reason=performance_unsupported\n'
    return 0
  fi

  # Saved once. Re-running apply must not overwrite the original value with
  # `performance` and make the restore path a no-op.
  if [[ ! -f "$state_file" ]]; then
    install -d -m 0755 "$state_root"
    install -m 0644 /dev/null "$state_file"
    cat "$cpu_root/cpu0/cpufreq/scaling_governor" >"$state_file"
  fi

  while IFS= read -r path; do
    [[ -w "$path" ]] || continue
    printf performance >"$path"
  done <<<"$paths"

  status
}

restore() {
  local paths path saved
  paths="$(governor_paths)"
  if [[ -z "$paths" || ! -f "$state_file" ]]; then
    printf 'CPU_GOVERNOR_RESTORE_SKIPPED\n'
    return 0
  fi
  saved="$(cat "$state_file")"
  if ! grep -qw "$saved" <<<"$(available_governors)"; then
    printf 'CPU_GOVERNOR_RESTORE_SKIPPED reason=saved_governor_unsupported saved=%s\n' "$saved"
    return 0
  fi
  while IFS= read -r path; do
    [[ -w "$path" ]] || continue
    printf '%s' "$saved" >"$path"
  done <<<"$paths"
  rm -f "$state_file"
  printf 'CPU_GOVERNOR_RESTORED governor=%s\n' "$saved"
}

case "$action" in
  apply) apply ;;
  restore) restore ;;
  status) status ;;
  *) usage ;;
esac
