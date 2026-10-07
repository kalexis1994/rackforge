#!/usr/bin/env bash
# What this machine is, read from it. Profiles decide from these facts and
# never assume them: a module that concerns a Pi 5, a display or Bluetooth
# acts only when the fact says so.

# Sets HW_* variables. Under $DEDICATED_ROOT for the self-test.
detect_hardware() {
  local model
  model="$(tr -d '\0' <"$DEDICATED_ROOT/proc/device-tree/model" 2>/dev/null || true)"
  HW_MODEL="${model:-unknown}"
  case "$HW_MODEL" in
    "Raspberry Pi 5"* | "Raspberry Pi 500"* | "Raspberry Pi Compute Module 5"*) HW_BOARD=pi5 ;;
    "Raspberry Pi 4"* | "Raspberry Pi 400"* | "Raspberry Pi Compute Module 4"*) HW_BOARD=pi4 ;;
    *) HW_BOARD=unknown ;;
  esac
  # A fact that cannot be read is unknown, never a reason to stop: this runs
  # under `set -e`.
  HW_CPUS="$(grep -c '^processor' "$DEDICATED_ROOT/proc/cpuinfo" 2>/dev/null || true)"
  HW_CPUS="${HW_CPUS:-0}"
  HW_MEMORY_MIB="$(awk '/^MemTotal:/ { print int($2 / 1024) }' "$DEDICATED_ROOT/proc/meminfo" 2>/dev/null || true)"
  HW_MEMORY_MIB="${HW_MEMORY_MIB:-0}"
  HW_OS="$(sed -n 's/^VERSION_CODENAME=//p' "$DEDICATED_ROOT/etc/os-release" 2>/dev/null || true)"
  HW_OS="${HW_OS:-unknown}"
  HW_KERNEL="$(uname -r)"
  HW_PREEMPT_RT=0
  [[ "$(cat "$DEDICATED_ROOT/sys/kernel/realtime" 2>/dev/null)" == 1 ]] && HW_PREEMPT_RT=1

  # Peripherals in use, not merely present: a Wi-Fi link that carries the
  # interface, a display that is connected, a camera that is attached.
  HW_WIFI_IN_USE=0
  if command -v nmcli >/dev/null 2>&1 && [[ -z "$DEDICATED_ROOT" ]]; then
    nmcli -t -f TYPE,STATE device 2>/dev/null | grep -q '^wifi:connected' && HW_WIFI_IN_USE=1
  fi
  HW_ETHERNET_IN_USE=0
  if [[ -z "$DEDICATED_ROOT" ]] && command -v nmcli >/dev/null 2>&1; then
    nmcli -t -f TYPE,STATE device 2>/dev/null | grep -q '^ethernet:connected' && HW_ETHERNET_IN_USE=1
  fi
  HW_DISPLAY_CONNECTED=0
  local status
  for status in "$DEDICATED_ROOT"/sys/class/drm/card*-*/status; do
    [[ -f "$status" && "$(cat "$status")" == connected ]] && HW_DISPLAY_CONNECTED=1
  done
  HW_CAMERA=0
  compgen -G "$DEDICATED_ROOT/dev/video*" >/dev/null && \
    grep -qs -i 'unicam\|rp1-cfe' "$DEDICATED_ROOT"/sys/class/video4linux/*/name && HW_CAMERA=1
  HW_BLUETOOTH_PAIRED=0
  compgen -G "$DEDICATED_ROOT/var/lib/bluetooth/*/*:*" >/dev/null && HW_BLUETOOTH_PAIRED=1

  # Where the USB interrupt lands is where the audio interface's
  # completions are handled. The Pi 4's controller sits behind PCIe and its
  # MSI cannot leave CPU 0 (measured). The Pi 5's sits behind RP1; not
  # measured, so not claimed.
  HW_USB_IRQ_CPU=""
  [[ "$HW_BOARD" == pi4 ]] && HW_USB_IRQ_CPU=0
  return 0
}

# Whether this boot is a trial: the firmware booted from tryboot.txt.
booted_on_trial() {
  [[ "$(od -An -tu4 --endian=big "$DEDICATED_ROOT/proc/device-tree/chosen/bootloader/tryboot" 2>/dev/null | tr -d ' ')" == 1 ]]
}

# Whether the kernel can mount the root filesystem by itself: the root is
# ext4 on an SD card or a USB disk, with both built into the kernel, and
# nothing -- encryption, LVM -- needs the initramfs to assemble it.
root_mountable_without_initramfs() {
  local config="$DEDICATED_ROOT/boot/config-$HW_KERNEL" device type
  [[ -f "$config" ]] || return 1
  grep -q '^CONFIG_EXT4_FS=y' "$config" || return 1
  if [[ -n "$DEDICATED_ROOT" ]]; then
    device="${HW_ROOT_DEVICE:-}"
    type="${HW_ROOT_TYPE:-}"
  else
    device="$(findmnt -no SOURCE / 2>/dev/null)"
    type="$(findmnt -no FSTYPE / 2>/dev/null)"
  fi
  [[ "$type" == ext4 ]] || return 1
  case "$device" in
    /dev/mmcblk*) grep -q '^CONFIG_MMC_BLOCK=y' "$config" || return 1 ;;
    /dev/sd*) grep -q '^CONFIG_USB_STORAGE=y' "$config" || return 1 ;;
    *) return 1 ;;
  esac
  # Anything listed to unlock or assemble at boot needs the initramfs.
  ! grep -qsE '^[[:space:]]*[^#[:space:]]' "$DEDICATED_ROOT/etc/crypttab"
}

print_hardware() {
  printf 'board        %s (%s)\n' "$HW_BOARD" "$HW_MODEL"
  printf 'cpus         %s\n' "$HW_CPUS"
  printf 'memory       %s MiB\n' "$HW_MEMORY_MIB"
  printf 'os           %s, kernel %s%s\n' "$HW_OS" "$HW_KERNEL" "$([[ $HW_PREEMPT_RT == 1 ]] && echo ' (PREEMPT_RT)')"
  printf 'network      wifi in use=%s ethernet in use=%s\n' "$HW_WIFI_IN_USE" "$HW_ETHERNET_IN_USE"
  printf 'display      connected=%s\n' "$HW_DISPLAY_CONNECTED"
  printf 'camera       %s\n' "$HW_CAMERA"
  printf 'bluetooth    paired devices=%s\n' "$HW_BLUETOOTH_PAIRED"
}
