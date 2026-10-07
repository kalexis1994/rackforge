#!/usr/bin/env bash
# Boot: what the firmware reads before the kernel starts.
#
# A change here decides whether the Pi boots at all, so it is never written
# straight into config.txt. It goes to tryboot.txt first; `rackforge-pi
# try-boot` restarts once from that file; a unit on that boot checks that
# the engine came up and only then makes the change permanent. A trial
# that fails, or hangs, is undone by the next power cycle: the firmware
# goes back to config.txt by itself.
#
# Measured on a Pi 4 (DEDICATED.md, D3): without the initramfs the firmware
# no longer reads its 11.8 MB, the root is mounted 0.65 s sooner and
# checked by systemd-fsck-root instead, and power to sound falls about
# 1.8 s.

BOOT_CONFIG=/boot/firmware/config.txt
BOOT_TRIAL=/boot/firmware/tryboot.txt
BOOT_TRIAL_PASSED="$STATE_DIR/boot-trial-passed"

# The lines this profile wants in config.txt, for this machine.
boot_lines() {
  local initramfs="$INITRAMFS"
  if [[ "$initramfs" == auto ]]; then
    initramfs="$(root_mountable_without_initramfs && echo off || echo keep)"
  fi
  [[ "$initramfs" == off ]] && echo "auto_initramfs=0"
  return 0
}

# What a trial proves: these lines, with this kernel, on this board. A new
# kernel or another line needs a trial of its own.
boot_trial_key() {
  printf '%s\n%s\n%s\n' "$(boot_lines)" "$HW_KERNEL" "$HW_MODEL" | sha256sum | cut -d' ' -f1
}

boot_trial_passed() {
  [[ "$(cat "$BOOT_TRIAL_PASSED" 2>/dev/null)" == "$(boot_trial_key)" ]]
}

boot_lines_in_config() {
  local line
  while read -r line; do
    [[ -z "$line" ]] && continue
    grep -qxF -- "$line" "$(path "$BOOT_CONFIG")" 2>/dev/null || return 1
  done < <(boot_lines)
  return 0
}

module_boot_declare() {
  local lines
  lines="$(boot_lines)"
  [[ -n "$lines" ]] || return 0
  [[ -f "$(path "$BOOT_CONFIG")" ]] || return 0
  if boot_trial_passed || boot_lines_in_config; then
    # Proven on this machine, or already in place: config.txt carries them.
    local line
    while read -r line; do
      [[ -n "$line" ]] && want_line "$BOOT_CONFIG" "$line"
    done <<<"$lines"
    return 0
  fi
  # Not yet proven: staged for a trial boot. The firmware reads tryboot.txt
  # instead of config.txt on that boot alone.
  want_file "$BOOT_TRIAL" 755 < <(
    cat "$(path "$BOOT_CONFIG")"
    printf '\n# Trial by rackforge-pi (dedicated profile).\n[all]\n%s\n' "$lines"
  )
  # The unit that checks the trial boot and makes it permanent.
  want_file /etc/systemd/system/rackforge-boot-trial.service 644 <<EOF
# Installed by rackforge-pi (dedicated profile).
[Unit]
Description=RackForge boot trial: keep a trial boot's configuration if the instrument came up
After=rackforge-audio.service
ConditionPathExists=$BOOT_TRIAL

[Service]
Type=oneshot
Environment=RACKFORGE_USER=$RACKFORGE_USER_RESOLVED
Environment=RACKFORGE_ROOT=$RACKFORGE_ROOT_RESOLVED
ExecStart=$RACKFORGE_PI_PATH boot-trial-check

[Install]
WantedBy=multi-user.target
EOF
  want_unit rackforge-boot-trial.service enabled
}

module_boot_check() {
  local lines
  lines="$(boot_lines)"
  [[ -n "$lines" ]] || return 0
  if ! boot_lines_in_config; then
    # Not a failure: a change that waits for its trial.
    echo "  PENDING [boot] $(tr '\n' ' ' <<<"$lines")waits for a trial boot: rackforge-pi try-boot"
  fi
  return 0
}

# Run by rackforge-boot-trial.service on every boot while a trial is staged.
boot_trial_check() {
  booted_on_trial || {
    echo "not a trial boot; the trial is still staged"
    return 0
  }
  # The instrument must be up: the engine running and its first period
  # played, within a minute of the boot.
  local waited=0
  # (A scratch tree for the self-test has no engine to wait for.)
  until [[ -n "$DEDICATED_ROOT" ]] || {
    "$DEDICATED_SYSTEMCTL" is-active --quiet rackforge-audio.service &&
      journalctl -b -u rackforge-audio -o cat --no-pager 2>/dev/null | grep -q '^READY_TO_PLAY'
  }; do
    if ((waited >= 60)); then
      echo "BOOT_TRIAL_FAILED the engine did not play within 60 s; config.txt is unchanged"
      return 1
    fi
    sleep 2
    waited=$((waited + 2))
  done
  mkdir -p "$STATE_DIR"
  boot_trial_key >"$BOOT_TRIAL_PASSED"
  echo "BOOT_TRIAL_PASSED $(boot_lines | tr '\n' ' ')"
}
