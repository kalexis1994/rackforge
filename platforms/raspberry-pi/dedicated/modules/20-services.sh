#!/usr/bin/env bash
# Services and timers: what runs besides the instrument.
#
# Kept, whatever the profile: audio, MIDI and the controllers (RackForge's
# own units), the interface (rackforge-web), administration (ssh,
# NetworkManager, avahi so that rackforge.local resolves, timesyncd), and
# the console. Everything else is either unused on an instrument or work
# that must not start in the middle of a performance.

# Jobs that run on a schedule and do real I/O or CPU work: package updates,
# filesystem scrubs and trims, the manual-page index, the package database
# backup. With MAINTENANCE=manual they run only through
# `rackforge-pi maintenance`.
SCHEDULED_MAINTENANCE=(
  apt-daily.timer
  apt-daily-upgrade.timer
  man-db.timer
  e2scrub_all.timer
  fstrim.timer
  dpkg-db-backup.timer
)

module_services_declare() {
  if [[ "$MAINTENANCE" == manual ]]; then
    local timer
    for timer in "${SCHEDULED_MAINTENANCE[@]}"; do
      [[ "$(unit_state "$timer")" == not-found ]] || want_unit "$timer" disabled now
    done
    # 1.1 s of every boot spent asking whether the bootloader has an update.
    [[ "$(unit_state rpi-eeprom-update.service)" == not-found ]] ||
      want_unit rpi-eeprom-update.service disabled
  fi

  # 1.2 s of every boot looking for leftover e2scrub snapshots, which exist
  # only on LVM; Raspberry Pi OS has none.
  [[ "$(unit_state e2scrub_reap.service)" == not-found ]] ||
    want_unit e2scrub_reap.service disabled

  # Mounting disks as they are plugged in, for a desktop. Started on demand
  # over D-Bus when something asks, so disabling it removes only the boot
  # start.
  [[ "$(unit_state udisks2.service)" == not-found ]] ||
    want_unit udisks2.service disabled now

  # Nothing on an instrument waits for the network to come up.
  [[ "$(unit_state NetworkManager-wait-online.service)" == not-found ]] ||
    want_unit NetworkManager-wait-online.service disabled

  local bluetooth="$BLUETOOTH"
  [[ "$bluetooth" == auto ]] && bluetooth="$([[ $HW_BLUETOOTH_PAIRED == 1 ]] && echo on || echo off)"
  if [[ "$bluetooth" == off && "$(unit_state bluetooth.service)" != not-found ]]; then
    want_unit bluetooth.service disabled now
  fi

  # cloud-init configured the image on its first boot; it has nothing to do
  # on an instrument's later ones but spend time looking.
  if command -v cloud-init >/dev/null 2>&1 && [[ -z "$DEDICATED_ROOT" ]] &&
    cloud-init status 2>/dev/null | grep -qE 'status: (done|disabled)'; then
    want_exists /etc/cloud/cloud-init.disabled
  fi
}

# Disabled is not the same as not running: a timer stopped by `disable
# --now` stays stopped, but one started by hand would not. What matters on
# stage is that none of them is active.
module_services_check() {
  local timer failed=0
  [[ "$MAINTENANCE" == manual ]] || return 0
  for timer in "${SCHEDULED_MAINTENANCE[@]}"; do
    if [[ "$("$DEDICATED_SYSTEMCTL" is-active "$timer" 2>/dev/null)" == active ]]; then
      echo "  NOT APPLIED [services] $timer is active"
      failed=1
    fi
  done
  return "$failed"
}
