#!/usr/bin/env bash
# Plays the bench under several CPU placements, one after another, on the
# Pi itself. Everything a placement changes is volatile -- the engine's
# environment in a /run drop-in, the governor and interrupt affinity
# through sysfs and procfs -- and is put back when the script ends, so a
# reboot would undo it too.
#
#   sudo cpu-placements.sh LABEL RUNS PLAN PLACEMENT...
#
# A placement is NAME:GOVERNOR:AUDIO_CPUS:RENDER_CPUS:IRQ_CPUS[:WORKERS],
# with `-` for "leave as it is". IRQ_CPUS moves the storage, Wi-Fi and network
# interrupts; the Pi 4's USB interrupt cannot move from CPU 0.
#
#   sudo cpu-placements.sh d2 2 edge \
#     float:ondemand:-:-:- performance:performance:-:-:- \
#     apart:performance:3:0,1,2:-
set -euo pipefail

label="${1:?label}"
runs="${2:?runs}"
plan="${3:?plan}"
shift 3

user="${SUDO_USER:?run through sudo}"
home="$(getent passwd "$user" | cut -d: -f6)"
bench="$home/bench/rackforge-pi-bench"
records="$home/bench/records"
dropin_dir=/run/systemd/system/rackforge-audio.service.d
dropin="$dropin_dir/50-placement.conf"
movable_irqs="$(grep -E 'mmc|eth0|wlan|brcmf' /proc/interrupts | cut -d: -f1 | tr -d ' ')"

governors=()
for policy in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
  governors+=("$policy=$(cat "$policy")")
done
irq_affinity=()
for irq in $movable_irqs; do
  irq_affinity+=("$irq=$(cat "/proc/irq/$irq/smp_affinity_list")")
done

restore() {
  rm -f "$dropin"
  for entry in "${governors[@]}"; do
    echo "${entry#*=}" > "${entry%%=*}"
  done
  for entry in "${irq_affinity[@]}"; do
    echo "${entry#*=}" > "/proc/irq/${entry%%=*}/smp_affinity_list" || true
  done
  systemctl daemon-reload
  systemctl restart rackforge-audio
  echo "restored"
}
trap restore EXIT

for placement in "$@"; do
  IFS=: read -r name governor audio render irqs workers <<< "$placement"
  workers="${workers:--}"
  for entry in "${governors[@]}"; do
    policy="${entry%%=*}"
    if [[ "$governor" == - ]]; then echo "${entry#*=}" > "$policy"; else echo "$governor" > "$policy"; fi
  done
  for entry in "${irq_affinity[@]}"; do
    irq="${entry%%=*}"
    target="${entry#*=}"
    [[ "$irqs" != - ]] && target="$irqs"
    echo "$target" > "/proc/irq/$irq/smp_affinity_list" || true
  done
  mkdir -p "$dropin_dir"
  {
    echo "[Service]"
    [[ "$audio" != - ]] && echo "Environment=RACKFORGE_AUDIO_CPUS=$audio"
    [[ "$render" != - ]] && echo "Environment=RACKFORGE_RENDER_CPUS=$render"
    [[ "$workers" != - ]] && echo "Environment=RACKFORGE_AUDIO_WORKERS=$workers"
    true
  } > "$dropin"
  systemctl daemon-reload
  systemctl restart rackforge-audio
  sleep 10
  for run in $(seq 1 "$runs"); do
    out="$records/$label-$name-run$run"
    echo "== $name run $run"
    sudo -u "$user" "$bench" perform --plan "$plan" --hold 20 --out "$out.json" > "$out.log" 2>&1
    grep -E "^==|LATE|clean" "$out.log" | awk '{print "   " $0}' | cut -c1-110 | grep -E "==|LATE" || true
  done
done
