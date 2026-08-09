#!/usr/bin/env bash
set -euo pipefail

action="${1:-audit}"
state_root="${RACKFORGE_APPLIANCE_STATE:-/var/lib/rackforge/appliance}"
backup_root="$state_root/rollback"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/install-env.sh"
rackforge_resolve_install_environment
service_user="$RACKFORGE_USER_RESOLVED"
rackforge_root="$RACKFORGE_ROOT_RESOLVED"
source_root="${RACKFORGE_SOURCE_ROOT:-$(cd -- "$script_dir/../../.." && pwd)}"
systemd_source="$source_root/platforms/raspberry-pi/systemd"
cloud_disabled=/etc/cloud/cloud-init.disabled
wait_unit=NetworkManager-wait-online.service
netplan_policy=/lib/netplan/00-network-manager-all.yaml
tmpfiles_policy=/etc/tmpfiles.d/rackforge-appliance.conf
boot_tuned_marker="$state_root/boot-tuned"
realtime_marker="$state_root/realtime-tuned"
limits_policy=/etc/security/limits.d/rackforge-audio.conf
limits_source="$source_root/platforms/raspberry-pi/etc/security/limits.d/rackforge-audio.conf"
governor_script_source="$source_root/platforms/raspberry-pi/sbin/rackforge-cpu-performance.sh"
governor_script="$rackforge_root/sbin/rackforge-cpu-performance.sh"
governor_unit=rackforge-cpu-performance.service
# Swap providers differ by image generation and neither is guaranteed to exist.
# Debian 13 Raspberry Pi images use rpi-swap, whose units are `static` or
# produced by a generator: `systemctl disable` on them is a silent no-op that
# looks like it worked. Its supported knob is a drop-in setting Mechanism=none,
# which is also what makes the change cleanly reversible. Older images use
# dphys-swapfile, which is an ordinary unit.
swap_dropin_dir=/etc/rpi/swap.conf.d
swap_dropin="$swap_dropin_dir/10-rackforge-realtime.conf"
legacy_swap_unit=dphys-swapfile.service

boot_config_path() {
  if [[ -f /boot/firmware/config.txt ]]; then
    printf /boot/firmware/config.txt
  elif [[ -f /boot/config.txt ]]; then
    printf /boot/config.txt
  else
    return 1
  fi
}

usage() {
  printf 'usage: %s audit|apply|rollback\n' "$0" >&2
  exit 2
}

require_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    exec sudo --preserve-env=RACKFORGE_USER,RACKFORGE_ROOT,RACKFORGE_APPLIANCE_STATE,RACKFORGE_SOURCE_ROOT \
      bash "$0" "$action"
  fi
}

is_known_pi() {
  local model
  model="$(tr -d '\0' </proc/device-tree/model 2>/dev/null || true)"
  [[ "$model" == Raspberry\ Pi\ 4\ Model\ * || "$model" == Raspberry\ Pi\ 5\ Model\ * ]]
}

audit_realtime() {
  local realtime xrun governor swap_active
  # The host prints exactly one REALTIME_ line per start. Reading it back from
  # the journal is the only way to know whether the grant below actually took
  # effect, as opposed to merely having been configured.
  realtime="$(journalctl -b -o cat --no-pager 2>/dev/null |
    sed -n 's/^\(REALTIME_[A-Z]* .*\)$/\1/p' | tail -1)"
  xrun="$(journalctl -b -o cat --no-pager 2>/dev/null |
    sed -n 's/^XRUN_RECOVERED .*total=\([0-9]*\)$/\1/p' | tail -1)"
  governor="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || printf unavailable)"
  swap_active="$(awk 'NR > 1 { found = 1 } END { print (found ? "yes" : "no") }' /proc/swaps 2>/dev/null || printf unknown)"
  printf 'realtime_tuned=%s\n' "$([[ -f "$realtime_marker" ]] && printf yes || printf no)"
  printf 'realtime_limits=%s\n' "$([[ -e "$limits_policy" ]] && printf installed || printf missing)"
  printf 'realtime_status=%s\n' "${realtime:-unknown}"
  printf 'cpu_governor=%s\n' "$governor"
  printf 'governor_unit=%s\n' "$(systemctl is-enabled "$governor_unit" 2>/dev/null || printf absent)"
  printf 'swap_active=%s\n' "$swap_active"
  printf 'xruns_since_boot=%s\n' "${xrun:-0}"
}

audit() {
  local model os ready oled web boot_config camera_detect display_detect
  model="$(tr -d '\0' </proc/device-tree/model 2>/dev/null || printf unknown)"
  os="$(. /etc/os-release && printf '%s' "${PRETTY_NAME:-unknown}")"
  ready="$(journalctl -b -o short-monotonic --no-pager 2>/dev/null |
    sed -n 's/^\[ *\([^]]*\)\].* READY_TO_PLAY$/\1/p' | head -1)"
  oled="$(journalctl -b -o short-monotonic --no-pager 2>/dev/null |
    sed -n 's/^\[ *\([^]]*\)\].* OLED bajo control de RackForge:.*/\1/p' | head -1)"
  web="$(journalctl -b -o short-monotonic --no-pager 2>/dev/null |
    sed -n 's/^\[ *\([^]]*\)\].* RACKFORGE_WEB_READY .*/\1/p' | head -1)"
  boot_config="$(boot_config_path 2>/dev/null || true)"
  camera_detect="$(sed -n 's/^[[:space:]]*camera_auto_detect=//p' "$boot_config" 2>/dev/null | tail -1)"
  display_detect="$(sed -n 's/^[[:space:]]*display_auto_detect=//p' "$boot_config" 2>/dev/null | tail -1)"
  printf 'APPLIANCE_AUDIT profile=raspberry-pi-os-lite\n'
  printf 'hardware=%s\n' "$model"
  printf 'os=%s\n' "$os"
  printf 'optimized=%s\n' "$([[ -f "$state_root/applied" ]] && printf yes || printf no)"
  printf 'cloud_init=%s\n' "$([[ -e "$cloud_disabled" ]] && printf disabled || printf enabled)"
  printf 'wait_online=%s\n' "$(systemctl is-enabled "$wait_unit" 2>/dev/null || true)"
  printf 'camera_auto_detect=%s\n' "${camera_detect:-unset}"
  printf 'display_auto_detect=%s\n' "${display_detect:-unset}"
  printf 'ready_to_play_seconds=%s\n' "${ready:-unknown}"
  printf 'oled_ready_seconds=%s\n' "${oled:-unknown}"
  printf 'web_ready_seconds=%s\n' "${web:-unknown}"
  audit_realtime
  systemd-analyze 2>/dev/null || true
}

apply_headless_boot_tuning() {
  local boot_config
  boot_config="$(boot_config_path)" || {
    printf 'Raspberry Pi boot config was not found.\n' >&2
    exit 1
  }
  if [[ ! -f "$backup_root/config.txt" ]]; then
    cp -a "$boot_config" "$backup_root/config.txt"
    printf '%s\n' "$boot_config" >"$backup_root/config.path"
    chmod 0600 "$backup_root/config.path"
  fi
  sed -i \
    -e 's/^[[:space:]]*camera_auto_detect=1[[:space:]]*$/camera_auto_detect=0/' \
    -e 's/^[[:space:]]*display_auto_detect=1[[:space:]]*$/display_auto_detect=0/' \
    "$boot_config"
  install -m 0600 /dev/null "$boot_tuned_marker"
}

apply_realtime_tuning() {
  install -d -m 0700 "$backup_root"

  # Recorded before anything is changed, so rollback restores the machine's
  # own swap policy rather than an assumed default.
  if [[ ! -f "$backup_root/realtime.env" ]]; then
    if [[ -e "$limits_policy" ]]; then
      cp -a "$limits_policy" "$backup_root/rackforge-audio.conf"
    fi
    if [[ -e /etc/systemd/system/rackforge-audio.service ]]; then
      cp -a /etc/systemd/system/rackforge-audio.service \
        "$backup_root/rackforge-audio.service"
    fi
    {
      printf 'LIMITS_PREEXISTING=%q\n' \
        "$([[ -e "$limits_policy" ]] && printf yes || printf no)"
      printf 'SWAP_DROPIN_PREEXISTING=%q\n' \
        "$([[ -e "$swap_dropin" ]] && printf yes || printf no)"
      printf 'LEGACY_SWAP_PREEXISTING=%q\n' \
        "$(systemctl is-enabled "$legacy_swap_unit" 2>/dev/null || printf absent)"
    } >"$backup_root/realtime.env"
    chmod 0600 "$backup_root/realtime.env"
  fi

  # The limits file grants to @rackforge, so the group must exist before PAM
  # ever evaluates it.
  groupadd --system --force rackforge
  usermod -a -G rackforge "$service_user"

  install -d -m 0755 "$(dirname "$governor_script")"
  install -m 0755 "$governor_script_source" "$governor_script"
  install -d -m 0755 "$(dirname "$limits_policy")"
  install -m 0644 "$limits_source" "$limits_policy"
  rackforge_render_systemd_unit \
    "$systemd_source/$governor_unit" \
    "/etc/systemd/system/$governor_unit"

  # Refreshed here because the real-time grant lives in the audio unit itself;
  # leaving a stale unit installed would configure everything except the part
  # that actually takes effect.
  if [[ -f "$systemd_source/rackforge-audio.service" ]]; then
    rackforge_render_systemd_unit \
      "$systemd_source/rackforge-audio.service" \
      /etc/systemd/system/rackforge-audio.service
  fi

  # `mlockall` already pins RackForge's own pages, so swap cannot evict the
  # audio path itself. What remains is the zram writeback path, which pushes
  # compressed pages out to a file on the SD card on a schedule of its own —
  # storage I/O landing mid-performance on a board whose swap sits unused.
  # Disabled for that reason, not out of a blanket rule against swap.
  if [[ -d /etc/rpi ]]; then
    install -d -m 0755 "$swap_dropin_dir"
    cat >"$swap_dropin" <<'EOF'
# Installed by RackForge's appliance profile.
# Removing this file, or running optimize-appliance.sh rollback, restores the
# image's own swap policy.
[Main]
Mechanism=none
EOF
    chmod 0644 "$swap_dropin"
  fi
  if [[ "$(systemctl is-enabled "$legacy_swap_unit" 2>/dev/null || printf absent)" == enabled ]]; then
    systemctl disable --now "$legacy_swap_unit" >/dev/null 2>&1 || true
  fi
  # Re-runs the generators so the drop-in takes effect now, rather than leaving
  # a stale swap unit active until the next boot.
  systemctl daemon-reload
  swapoff --all >/dev/null 2>&1 || true

  systemctl daemon-reload
  systemctl enable "$governor_unit" >/dev/null
  systemctl start "$governor_unit" >/dev/null 2>&1 || true
  install -m 0600 /dev/null "$realtime_marker"
}

rollback_realtime_tuning() {
  test -f "$backup_root/realtime.env" || return 0

  # Defaulted before sourcing. A state file written by an earlier version of
  # this script will not carry every key, and under `set -u` that aborts the
  # rollback halfway — leaving the machine in the one state nobody chose.
  # Each default describes a file this script owns by construction, so
  # reverting it is safe even when the recorded history is missing.
  LIMITS_PREEXISTING=no
  SWAP_DROPIN_PREEXISTING=no
  LEGACY_SWAP_PREEXISTING=absent

  # Root-owned, mode 0600, and written only by this script.
  source "$backup_root/realtime.env"

  systemctl disable --now "$governor_unit" >/dev/null 2>&1 || true
  if [[ -x "$governor_script" ]]; then
    RACKFORGE_APPLIANCE_STATE="$state_root" bash "$governor_script" restore >/dev/null 2>&1 || true
  fi
  rm -f "/etc/systemd/system/$governor_unit" "$governor_script"

  if [[ "$LIMITS_PREEXISTING" == yes && -f "$backup_root/rackforge-audio.conf" ]]; then
    cp -a "$backup_root/rackforge-audio.conf" "$limits_policy"
  else
    rm -f "$limits_policy"
  fi

  if [[ -f "$backup_root/rackforge-audio.service" ]]; then
    cp -a "$backup_root/rackforge-audio.service" \
      /etc/systemd/system/rackforge-audio.service
  fi

  if [[ "$SWAP_DROPIN_PREEXISTING" == no ]]; then
    rm -f "$swap_dropin"
    rmdir --ignore-fail-on-non-empty "$swap_dropin_dir" 2>/dev/null || true
  fi
  if [[ "$LEGACY_SWAP_PREEXISTING" == enabled ]]; then
    systemctl enable --now "$legacy_swap_unit" >/dev/null 2>&1 || true
  fi

  rm -f "$realtime_marker"
}

check_provisioned() {
  local cloud_status
  is_known_pi || {
    printf 'Refusing appliance profile: Raspberry Pi 4/5 was not detected.\n' >&2
    exit 1
  }
  command -v nmcli >/dev/null
  command -v cloud-init >/dev/null
  getent passwd "$service_user" >/dev/null || {
    printf 'Provisioned user %s does not exist.\n' "$service_user" >&2
    exit 1
  }
  find /etc/ssh -maxdepth 1 -name 'ssh_host_*_key' -type f -print -quit | grep -q . || {
    printf 'SSH host keys are missing; first-boot provisioning is incomplete.\n' >&2
    exit 1
  }
  nmcli -t -f TYPE connection show | grep -Eq '^(802-3-ethernet|802-11-wireless)$' || {
    printf 'No persistent Ethernet or Wi-Fi profile exists.\n' >&2
    exit 1
  }
  cloud_status="$(cloud-init status --long 2>/dev/null || true)"
  if ! grep -Eq '^status: done$' <<<"$cloud_status"; then
    # Rollback removes the disable marker, after which cloud-init reports
    # `not started` until the next boot re-runs it. Recorded rollback state is
    # proof this machine was already provisioned, so an apply/rollback/apply
    # round trip must not be mistaken for a machine that never booted.
    if [[ ! -f "$backup_root/state.env" ]]; then
      printf 'cloud-init has not completed its first boot.\n' >&2
      exit 1
    fi
    printf 'APPLIANCE_PREVIOUSLY_PROVISIONED cloud_init=%s\n' \
      "$(sed -n 's/^status: //p' <<<"$cloud_status" | head -1)"
  fi
  test -f "$systemd_source/rackforge-platform-host.service"
  test -f "$systemd_source/rackforge-web.service"
  test -f "$systemd_source/$governor_unit"
  test -f "$limits_source"
  test -f "$governor_script_source"
}

apply_profile() {
  require_root
  if [[ -f "$state_root/applied" && -f "$boot_tuned_marker" && -f "$realtime_marker" ]]; then
    printf 'APPLIANCE_ALREADY_OPTIMIZED\n'
    audit
    return
  fi
  if [[ ! -f "$state_root/applied" ]]; then
    check_provisioned
    install -d -m 0700 "$backup_root"
    cp -a /etc/systemd/system/rackforge-platform-host.service \
      "$backup_root/rackforge-platform-host.service"
    cp -a /etc/systemd/system/rackforge-web.service \
      "$backup_root/rackforge-web.service"
    if [[ -e "$tmpfiles_policy" ]]; then
      cp -a "$tmpfiles_policy" "$backup_root/rackforge-appliance.conf"
      tmpfiles_preexisting=yes
    else
      tmpfiles_preexisting=no
    fi
    cloud_preexisting="$([[ -e "$cloud_disabled" ]] && printf yes || printf no)"
    wait_preexisting="$(systemctl is-enabled "$wait_unit" 2>/dev/null || true)"
    netplan_mode="$([[ -e "$netplan_policy" ]] && stat -c '%a' "$netplan_policy" || printf missing)"
    {
      printf 'CLOUD_PREEXISTING=%q\n' "$cloud_preexisting"
      printf 'WAIT_PREEXISTING=%q\n' "$wait_preexisting"
      printf 'NETPLAN_MODE=%q\n' "$netplan_mode"
      printf 'TMPFILES_PREEXISTING=%q\n' "$tmpfiles_preexisting"
    } >"$backup_root/state.env"
    chmod 0600 "$backup_root/state.env"

    rackforge_render_systemd_unit \
      "$systemd_source/rackforge-platform-host.service" \
      /etc/systemd/system/rackforge-platform-host.service
    rackforge_render_systemd_unit \
      "$systemd_source/rackforge-web.service" \
      /etc/systemd/system/rackforge-web.service
    install -m 0644 /dev/null "$cloud_disabled"
    cat >"$tmpfiles_policy" <<'EOF'
# Netplan contains network policy and rejects world-readable configuration.
z /lib/netplan/00-network-manager-all.yaml 0600 root root -
EOF
    chmod 0644 "$tmpfiles_policy"
    if [[ -e "$netplan_policy" ]]; then
      chmod 0600 "$netplan_policy"
    fi
    systemctl disable "$wait_unit" >/dev/null 2>&1 || true
    systemctl daemon-reload
    install -m 0600 /dev/null "$state_root/applied"
  fi
  apply_headless_boot_tuning
  apply_realtime_tuning
  printf 'APPLIANCE_OPTIMIZED reboot_required=yes rollback=%s\n' "$0 rollback"
}

rollback_profile() {
  require_root
  test -f "$backup_root/state.env" || {
    printf 'No RackForge appliance rollback state exists.\n' >&2
    exit 1
  }
  # Defaulted for the same reason as the real-time state: a state file from an
  # earlier version must not abort the rollback partway through.
  CLOUD_PREEXISTING=no
  WAIT_PREEXISTING=disabled
  NETPLAN_MODE=missing
  TMPFILES_PREEXISTING=no

  # Root-owned, mode 0600, and written only by this script.
  source "$backup_root/state.env"
  cp -a "$backup_root/rackforge-platform-host.service" \
    /etc/systemd/system/rackforge-platform-host.service
  cp -a "$backup_root/rackforge-web.service" \
    /etc/systemd/system/rackforge-web.service
  if [[ "$CLOUD_PREEXISTING" == no ]]; then
    rm -f "$cloud_disabled"
  fi
  if [[ "$TMPFILES_PREEXISTING" == yes ]]; then
    cp -a "$backup_root/rackforge-appliance.conf" "$tmpfiles_policy"
  else
    rm -f "$tmpfiles_policy"
  fi
  if [[ "$NETPLAN_MODE" != missing && -e "$netplan_policy" ]]; then
    chmod "$NETPLAN_MODE" "$netplan_policy"
  fi
  if [[ -f "$backup_root/config.txt" && -f "$backup_root/config.path" ]]; then
    boot_config="$(cat "$backup_root/config.path")"
    cp -a "$backup_root/config.txt" "$boot_config"
  fi
  if [[ "$WAIT_PREEXISTING" == enabled ]]; then
    systemctl enable "$wait_unit" >/dev/null
  else
    systemctl disable "$wait_unit" >/dev/null 2>&1 || true
  fi
  rollback_realtime_tuning
  systemctl daemon-reload
  rm -f "$state_root/applied" "$boot_tuned_marker"
  printf 'APPLIANCE_ROLLED_BACK reboot_required=yes\n'
}

case "$action" in
  audit) audit ;;
  apply) apply_profile ;;
  rollback) rollback_profile ;;
  *) usage ;;
esac
