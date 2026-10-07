#!/usr/bin/env bash
# Runs rackforge-pi, every module included, against a scratch copy of a Pi:
# plan, apply, verify, apply again, revert -- and checks that revert leaves
# the tree exactly as it was. Needs a Linux userland (getent, GNU tools);
# runs on the Pi or any Linux machine, as an ordinary user.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cli="$here/../rackforge-pi"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
export DEDICATED_ROOT="$scratch/root"
export DEDICATED_SYSTEMCTL="$here/fake-systemctl"
export FAKE_UNITS="$scratch/units"
export RACKFORGE_USER="$(id -un)"
units="$FAKE_UNITS"
root="$DEDICATED_ROOT"
mkdir -p "$units" "$root/proc/device-tree" "$root/etc/rpi/swap.conf.d" "$root/etc/systemd/system" \
  "$root/etc/security/limits.d" "$root/boot/firmware" "$root/sys/class/drm"

printf 'Raspberry Pi 4 Model B Rev 1.4\0' >"$root/proc/device-tree/model"
printf 'processor\t: 0\nprocessor\t: 1\nprocessor\t: 2\nprocessor\t: 3\n' >"$root/proc/cpuinfo"
printf 'MemTotal:        8007004 kB\n' >"$root/proc/meminfo"
printf 'VERSION_CODENAME=trixie\n' >"$root/etc/os-release"
# A kernel that mounts an ext4 root on the SD card by itself, so the boot
# module stages dropping the initramfs for a trial.
mkdir -p "$root/boot" "$root/proc/device-tree/chosen/bootloader"
printf 'CONFIG_EXT4_FS=y\nCONFIG_MMC_BLOCK=y\n' >"$root/boot/config-$(uname -r)"
export HW_ROOT_DEVICE=/dev/mmcblk0p2 HW_ROOT_TYPE=ext4
printf 'dtparam=audio=on\ncamera_auto_detect=0\ndisplay_auto_detect=0\n[pi5]\ndtoverlay=nospi10\n[all]\n' >"$root/boot/firmware/config.txt"
# Without a final newline, as Raspberry Pi OS writes it (the engine's own
# self-test covers the other ending).
printf 'console=tty1 root=PARTUUID=1 rootwait' >"$root/boot/firmware/cmdline.txt"
printf '[Main]\nMechanism=none\n' >"$root/etc/rpi/swap.conf.d/10-rackforge-realtime.conf"
for unit in apt-daily.timer apt-daily-upgrade.timer man-db.timer e2scrub_all.timer fstrim.timer \
  dpkg-db-backup.timer rpi-eeprom-update.service e2scrub_reap.service udisks2.service \
  bluetooth.service; do
  echo enabled >"$units/$unit"
done
echo disabled >"$units/NetworkManager-wait-online.service"

snapshot() { (cd "$root" && find . -path ./var -prune -o -type f -print0 | sort -z | xargs -0 sha256sum); }
# Units RackForge brings are left disabled on revert; the rest must be back.
units_snapshot() { (cd "$units" && grep -H . *) | grep -v '^rackforge-'; }
fails() { ! "$@" >/dev/null; }
before="$(snapshot)"
units_before="$(units_snapshot)"

failures=0
check() {
  local what="$1"
  shift
  if "$@"; then printf 'ok    %s\n' "$what"; else printf 'FAIL  %s\n' "$what"; failures=$((failures + 1)); fi
}

"$cli" status
"$cli" plan
check "apply succeeds and verifies" "$cli" apply
check "verify passes afterwards" "$cli" verify
check "a second apply changes nothing" bash -c "! '$cli' apply | grep -q '^applied:'"
check "timers are disabled" grep -qx disabled "$units/apt-daily.timer"
check "bluetooth is off without a paired device" grep -qx disabled "$units/bluetooth.service"
check "the governor unit is enabled" grep -qx enabled "$units/rackforge-cpu-performance.service"
check "the journal is volatile" grep -qx 'Storage=volatile' "$root/etc/systemd/journald.conf.d/90-rackforge.conf"
check "a boot change waits in tryboot.txt" grep -qx 'auto_initramfs=0' "$root/boot/firmware/tryboot.txt"
check "config.txt is untouched before the trial" fails grep -qx 'auto_initramfs=0' "$root/boot/firmware/config.txt"
check "verify says the change is pending" bash -c "'$cli' verify | grep -q PENDING"
# The trial boot: the firmware says so, and the engine played.
printf '\0\0\0\1' >"$root/proc/device-tree/chosen/bootloader/tryboot"
"$cli" boot-trial-check >/dev/null 2>&1 || true
check "a passed trial moves the change into config.txt" grep -qx 'auto_initramfs=0' "$root/boot/firmware/config.txt"
check "and clears tryboot.txt" test ! -e "$root/boot/firmware/tryboot.txt"
rm -f "$root/proc/device-tree/chosen/bootloader/tryboot"
"$cli" revert
check "revert restores every file" test "$(snapshot)" == "$before"
check "revert restores every unit" test "$(units_snapshot)" == "$units_before"
check "a unit RackForge brought is disabled before its file goes" \
  grep -qx disabled "$units/rackforge-cpu-performance.service"

((failures == 0)) && echo "all checks passed" || { echo "$failures checks failed"; exit 1; }
