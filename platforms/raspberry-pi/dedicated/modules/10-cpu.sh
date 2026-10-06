#!/usr/bin/env bash
# CPU: the governor, where the engine's threads run, and where the
# interrupts it does not need land.
#
# Measured on the board each `auto` stands for (DEDICATED.md, D2); a board
# nobody measured keeps the scheduler's placement, which is never worse
# than a guess.

# Resolves AUDIO_CPUS, RENDER_CPUS and IRQ_CPUS to lists, or empty for
# "leave it to the scheduler", and AUDIO_WORKERS to a count, or empty for
# the engine's own choice.
#
# On a Pi 4 (D2): pinning any engine thread, the audio thread included,
# doubled Concert Grand's peak and brought dropouts past 20 voices; the
# scheduler, free to move a worker off a busy core, did better than every
# fixed placement tried. Four workers -- one per core, where the engine's
# default keeps one core back -- gave Concert Grand's four units a core each
# and took its peak from 97-149 % to 58-104 %. Moving the storage and
# network interrupts changed nothing measurable.
cpu_placement() {
  CPU_AUDIO="$AUDIO_CPUS"
  CPU_RENDER="$RENDER_CPUS"
  CPU_IRQS="$IRQ_CPUS"
  CPU_WORKERS="$AUDIO_WORKERS"
  case "$HW_BOARD" in
    pi4) [[ "$CPU_WORKERS" == auto ]] && CPU_WORKERS="$HW_CPUS" ;;
  esac
  # Anything still `auto` is a board nobody measured: the scheduler and the
  # engine's own worker count, never a guess.
  [[ "$CPU_AUDIO" == auto ]] && CPU_AUDIO=""
  [[ "$CPU_RENDER" == auto ]] && CPU_RENDER=""
  [[ "$CPU_IRQS" == auto || "$CPU_IRQS" == keep ]] && CPU_IRQS=""
  [[ "$CPU_WORKERS" == auto ]] && CPU_WORKERS=""
  return 0
}

module_cpu_declare() {
  cpu_placement
  # The name the engine's unit already asks for (Wants= and After=), so the
  # governor is in force before the first period.
  want_file /etc/systemd/system/rackforge-cpu-performance.service 644 <<EOF
# Installed by rackforge-pi (dedicated profile).
[Unit]
Description=RackForge CPU placement: governor and interrupts
DefaultDependencies=no
After=sysinit.target
Before=rackforge-audio.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c 'for policy in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do echo $GOVERNOR > "\$\$policy"; done'
$(
    if [[ -n "$CPU_IRQS" ]]; then
      printf '%s\n' "ExecStart=/bin/sh -c 'for irq in \$\$(grep -E \"mmc|eth0|wlan|brcmf\" /proc/interrupts | cut -d: -f1); do echo $CPU_IRQS > /proc/irq/\$\$irq/smp_affinity_list || true; done'"
    fi
)

[Install]
WantedBy=multi-user.target
EOF
  want_unit rackforge-cpu-performance.service enabled now

  if [[ -n "$CPU_AUDIO$CPU_RENDER$CPU_WORKERS" ]]; then
    # Redirected, not piped: a pipe would declare the file in a subshell,
    # and the declaration would be lost with it.
    want_file /etc/systemd/system/rackforge-audio.service.d/40-dedicated-cpu.conf 644 < <(
      echo "# Installed by rackforge-pi (dedicated profile)."
      echo "[Service]"
      [[ -n "$CPU_AUDIO" ]] && echo "Environment=RACKFORGE_AUDIO_CPUS=$CPU_AUDIO"
      [[ -n "$CPU_RENDER" ]] && echo "Environment=RACKFORGE_RENDER_CPUS=$CPU_RENDER"
      [[ -n "$CPU_WORKERS" ]] && echo "Environment=RACKFORGE_AUDIO_WORKERS=$CPU_WORKERS"
      true
    )
  fi
}

module_cpu_after() {
  [[ -n "$DEDICATED_ROOT" ]] && return 0
  "$DEDICATED_SYSTEMCTL" restart rackforge-cpu-performance.service
  # The engine reads its placement when it starts.
  if "$DEDICATED_SYSTEMCTL" is-active --quiet rackforge-audio.service; then
    "$DEDICATED_SYSTEMCTL" restart rackforge-audio.service
  fi
}

# The governor in force, and the engine's threads as the kernel sees them:
# realtime only where it belongs, and on the CPUs the profile chose.
module_cpu_check() {
  [[ -n "$DEDICATED_ROOT" ]] && return 0
  cpu_placement
  local failed=0 policy pid task comm fields
  for policy in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
    if [[ "$(cat "$policy")" != "$GOVERNOR" ]]; then
      echo "  NOT APPLIED [cpu] ${policy%/cpufreq/*}: governor $(cat "$policy"), not $GOVERNOR"
      failed=1
    fi
  done
  pid="$("$DEDICATED_SYSTEMCTL" show -p MainPID --value rackforge-audio 2>/dev/null || true)"
  [[ -n "$pid" && "$pid" != 0 && -d "/proc/$pid" ]] || return "$failed"
  for task in /proc/"$pid"/task/*; do
    comm="$(cat "$task/comm")"
    # Policy is field 41 of stat, counted after the parenthesised name.
    fields="$(sed 's/^.*) //' "$task/stat")"
    local policy_number allowed
    policy_number="$(awk '{ print $39 }' <<<"$fields")"
    allowed="$(awk '/^Cpus_allowed_list:/ { print $2 }' "$task/status")"
    if [[ "$policy_number" == 1 || "$policy_number" == 2 ]]; then
      if [[ "$(basename "$task")" != "$pid" && "$comm" != rf-render-* ]]; then
        echo "  NOT APPLIED [cpu] engine thread $comm runs realtime and should not"
        failed=1
      fi
    fi
    if [[ "$(basename "$task")" == "$pid" && -n "$CPU_AUDIO" && "$allowed" != "$CPU_AUDIO" ]]; then
      echo "  NOT APPLIED [cpu] the audio thread runs on $allowed, not $CPU_AUDIO"
      failed=1
    fi
  done
  return "$failed"
}
