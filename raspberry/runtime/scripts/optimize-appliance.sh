#!/usr/bin/env bash
set -euo pipefail

action="${1:-audit}"
service_user="${RACKFORGE_USER:-kalex}"
state_root="${RACKFORGE_APPLIANCE_STATE:-/var/lib/rackforge/appliance}"
backup_root="$state_root/rollback"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source_root="${RACKFORGE_SOURCE_ROOT:-$(cd -- "$script_dir/../.." && pwd)}"
systemd_source="$source_root/systemd"
cloud_disabled=/etc/cloud/cloud-init.disabled
wait_unit=NetworkManager-wait-online.service
netplan_policy=/lib/netplan/00-network-manager-all.yaml
tmpfiles_policy=/etc/tmpfiles.d/rackforge-appliance.conf
boot_tuned_marker="$state_root/boot-tuned"

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
    exec sudo --preserve-env=RACKFORGE_USER,RACKFORGE_APPLIANCE_STATE,RACKFORGE_SOURCE_ROOT \
      bash "$0" "$action"
  fi
}

is_known_pi() {
  local model
  model="$(tr -d '\0' </proc/device-tree/model 2>/dev/null || true)"
  [[ "$model" == Raspberry\ Pi\ 4\ Model\ * || "$model" == Raspberry\ Pi\ 5\ Model\ * ]]
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
  grep -Eq '^status: done$' <<<"$cloud_status" || {
    printf 'cloud-init has not completed its first boot.\n' >&2
    exit 1
  }
  test -f "$systemd_source/rackforge-platform-host.service"
  test -f "$systemd_source/rackforge-web.service"
}

apply_profile() {
  require_root
  if [[ -f "$state_root/applied" && -f "$boot_tuned_marker" ]]; then
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

    install -m 0644 "$systemd_source/rackforge-platform-host.service" \
      /etc/systemd/system/rackforge-platform-host.service
    install -m 0644 "$systemd_source/rackforge-web.service" \
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
  printf 'APPLIANCE_OPTIMIZED reboot_required=yes rollback=%s\n' "$0 rollback"
}

rollback_profile() {
  require_root
  test -f "$backup_root/state.env" || {
    printf 'No RackForge appliance rollback state exists.\n' >&2
    exit 1
  }
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
