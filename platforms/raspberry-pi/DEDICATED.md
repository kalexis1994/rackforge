# Dedicated mode: the Pi as an instrument

Status: plan. Nothing here is implemented yet; each milestone states what it
must show before the next one starts.

The normal installation puts RackForge on a Raspberry Pi OS Lite system and
leaves the system as it was. **Dedicated** mode is an explicit second mode: it
transforms the system so that the Pi boots into RackForge, restores the
session, and runs the engine before anything else. It may change services,
packages, boot configuration, CPU and memory policy, storage, and, when a
measurement justifies it, the kernel. Bash coordinates the transformation;
what the engine has to do differently is done in the engine.

## Where we start (measured 2026-10-06)

Pi 4 Model B Rev 1.4, 8 GB, Debian 13 (trixie) Raspberry Pi OS Lite,
kernel `6.18.39+rpt-rpi-v8` (`PREEMPT`, not `PREEMPT_RT`), Scarlett Solo 3rd
gen and KeyLab Essential 61 mk3 behind a VIA hub, 48 kHz, 256/768 frames.
RackForge `5c54baa` installed with `install.sh` and `install-appliance.sh`,
without `--optimize`.

What the running system actually does, against what the installer intends:

| Area | Intended | Observed |
| --- | --- | --- |
| Governor | `performance` before audio | `ondemand` on every core; `rackforge-cpu-performance.service` is not installed, yet `rackforge-audio.service` wants it |
| Audio thread | FIFO 75 | FIFO 75 (it is the process's main thread) |
| Render workers | FIFO 74, 3 of them | FIFO 74 ×3, no affinity: with the audio thread, all four cores carry a FIFO thread |
| Telemetry thread | normal | **FIFO 75**: spawned after the audio thread engaged, so it inherited it; it formats lines and writes files |
| Memory lock | `mlockall` | `VmLck` 147 GB (wasm reservations, never touched), RSS 408 MB; the two native control threads' 64 MiB stacks are committed |
| Epoch tickers | — | 10 threads waking at 1 kHz, one per wasm plugin engine |
| IRQs | — | xHCI (USB: interface and keyboard), eth0 and mmc all on CPU 0, threaded IRQs at FIFO 50 |
| Swap | none | `rpi-swap` drop-in `Mechanism=none`; zram0 exists, no swap active |
| Journal | — | volatile (nothing under `/var/log/journal`), default size limits |
| Services | — | bluetooth, avahi, udisks2, rpi-connect-lite, cron, apt-daily/-upgrade, man-db, e2scrub, rpi-eeprom-update, dpkg-db-backup, fstrim timers all active |
| Boot | sound without network | kernel 2.8 s + userspace; `READY` at 9.7 s after kernel start; `REALTIME_ENGAGED` at 11.2 s; NetworkManager 6.0 s |

The code review behind the table found, besides:

- The realtime audit greps the journal, and its pattern picks up
  `REALTIME_THROTTLING`/`REALTIME_REMEDY` instead of the status line; worker
  status is never read; no thread's real policy is ever checked.
- Xruns are counted in a local of the audio loop and lost on every restart;
  the audit labels the last `total=` of the boot "since boot". Input xruns are
  not counted. The Pi exposes no xrun count to the interface.
- `optimize-appliance.sh` trusts its `applied`/`boot-tuned`/`realtime-tuned`
  markers, overwrites an existing swap drop-in without saving it, keeps one
  `config.txt` backup forever and restores the whole file, and loses the
  original state when a first run is interrupted.
- Release recovery covers only a failure during the install: the backup is
  deleted on success, so there is no rollback afterwards, and a rollback
  restores neither plugin-store changes, config/state migrations nor OS state.
- On the audio thread: control commands that load presets, create instances
  or reopen ALSA run inline; native plugin calls block on a normal-priority
  control thread (priority inversion); `println!` once a second; the budget
  store's mutex is held across a file write by the telemetry thread.
- The audio supervisor exits the process when a "better" device appears;
  a capture failure exits it too.
- MIDI events all land at frame 0 of the block.

## How it will be judged

Every claim of improvement is a **before/after on the same Pi**, interface,
instruments, sample rate and buffer, from one tool, `rackforge-pi-bench`,
whose output is a JSON record kept with the release:

- **Xruns** (output and input) and **deadline misses** over a fixed scripted
  performance, and over a long idle soak.
- **Block time**: avg, p95, p99, max, against the deadline.
- **Stable polyphony**: per instrument, the most voices the scripted ramp
  reaches with no deadline miss and no xrun for 60 s.
- **Memory**: RSS, locked, major and minor faults during the performance
  (the faults must not grow once it plays).
- **Temperature and throttling** at the end of the soak.
- **Time to ready**: from kernel start to the engine's first block, to the
  interface answering on the network, to SSH; over 10 cold boots, median and
  worst. Firmware time before the kernel is noted separately.
- **Kernel latency**: `cyclictest` under the same load, for the kernel
  question only.

The scripted performance plays through the control socket, as the Touch
Controller does, so it needs no hands and no keyboard.

## The installer

One entry command, `rackforge-pi`, with subcommands:

```text
rackforge-pi status            what is applied, read from the system itself
rackforge-pi plan [--profile]  what apply would change, module by module
rackforge-pi apply             apply the plan, recording what it replaces
rackforge-pi verify            check every module's real state
rackforge-pi revert [module]   put back what was recorded
```

- **Modules** (`dedicated/modules/NN-name.sh`) each implement `probe` (read
  the real state), `want` (from the profile and the hardware), `apply`,
  `verify` and `revert`. `plan` is the difference between `probe` and `want`.
  A module never decides it is applied from a marker: `verify` reads the
  system.
- **Profiles** are versioned files (`dedicated/profiles/dedicated-1.conf`).
  Applying a newer profile version over an older one is an ordinary apply:
  the plan shows what changes.
- **State** lives in `/var/lib/rackforge/dedicated/`: for every file or
  setting a module touches, the original content (or its absence), recorded
  before the first change and never overwritten by a later run. A journal of
  started and finished steps lets an interrupted run resume or roll back on
  the next invocation.
- **Hardware** is detected, never assumed: board (Pi 4, Pi 5, 400/500,
  CM4/CM5), RAM, OS release, kernel, audio and MIDI devices, display,
  camera, Wi-Fi/Bluetooth in use. Modules that concern a peripheral act only
  on what the profile says is used.
- `install-release.sh --dedicated` installs the release and applies the
  profile; without it, the installation stays normal.

## Milestones

Each one states its prediction first and records its result here, met or
not.

### D0. Measure the starting point

`rackforge-pi-bench` and the scripted performance; the realtime and xrun
fixes needed to measure honestly (a self-audit of every engine thread's real
policy, affinity and stack; xruns counted per boot and in total, input
included, exposed on the control socket). Baseline recorded on the system as
it is today, and again after a plain `--optimize` install.

*Prediction:* the bench repeats within 5 % on block p99 and exactly on
xruns across three runs of the same configuration.

**Built.** The engine answers `AudioHealth` on the appliance: per-period
work against the period (all of it, not only the render), overruns,
underruns, capture xruns, stream errors and dropped MIDI, as totals for the
process and a window per read. Its threads are named `rf-*` within Linux's
15 bytes, so an audit can tell them apart. `bench/rackforge-pi-bench`
audits from `/proc` and `/sys`, plays the scripted ramp (1 to 24 voices,
each held 20 s and re-struck every 2 s, on Concert Grand, RF-Musette
Student 72, RF-5 Slow Strings and RF-Organ Straight 888), and compares;
`bench/boot-cycles.sh` reboots from another machine and records each boot.

**Baseline** (v0.1.29 plus that instrumentation, normal install, 48 kHz,
256/768, three runs):

| Instrument | Stable voices, nothing late | Without a dropout |
| --- | --- | --- |
| Concert Grand | 12, 12, 6 | 24, 24, 24 |
| RF-Musette Student 72 | 8, 6, 6 | 10, 10, 10 |
| RF-5 Slow Strings | 24, 24, 24 | 24, 24, 24 |
| RF-Organ Straight 888 | 24, 24, 24 | 24, 24, 24 |

- No page faults at all while playing; RSS 404 MB, 32 threads.
- 48–50 °C after each run, never throttled.
- Concert Grand's 6 is one block at 119 % with 8 voices in the third run:
  a stray spike, which is what D1 and D2 are about. RF-Musette above 8
  voices is not noise but work: 120–125 % of the period at 10 voices, 150 %
  at 12. No tuning of the system buys that back.
- Boot, median of 10 (worst): engine started 6.1 s after the kernel,
  first period played at 10.4 s (11.4), web and SSH at 17.2 s, network
  at 20.9 s. From SSH going silent to answering again took 33.9 s, so
  about 16.7 s go to the end of shutdown, the firmware and the
  bootloader, which the kernel cannot see.

**Result: partly met.** Heavy steps repeat exactly (RF-Musette at 10
voices: 16 overruns in each run; at 12: 3593, 3629, 3655). Light steps do
not: p99 moves up to 20 % between runs, partly the telemetry histogram's
~4 % buckets and partly stray spikes, and one spike moves the strict
stable-voice count. Comparisons therefore rest on dropouts, on the heavy
steps' counts and on repeated runs, never on one p99.

### D1. The engine's own house

Engine changes that any installation benefits from:

- telemetry and every helper thread spawned at normal priority, explicitly;
- one epoch ticker for all wasm engines;
- native control threads with stacks sized for what they run, and stacks
  touched before `mlockall` rather than committed by it;
- `mlockall` once, with `VmLck`/RSS verified and reported;
- wasmtime memory reservations bounded to what a plugin declares;
- heavy control commands prepared off the audio thread and swapped in;
- the supervisor never exits for a "better" device while playing;
- `EXTEND_TIMEOUT_USEC` while compiling, so a cold cache cannot trip the
  start timeout.

*Prediction:* RSS falls by at least the committed control stacks
(≥ 128 MB here); no FIFO thread other than the audio thread and the workers;
p99 no worse, xruns no more.

**Built, first pass:**

- the telemetry publisher leaves the realtime policy it inherited from the
  audio thread, and the CPUs if the audio thread was pinned;
- one pair of wasmtime engines for the process, so one epoch ticker and one
  cache worker pair instead of one per plugin;
- the budget store's lock released before the file is written;
- `EXTEND_TIMEOUT_USEC` before each plugin loads;
- the audio supervisor reads the card list every 2 s and probes the PCMs
  only for a few ticks after it changed, instead of opening every PCM of
  every card (the keyboard's USB audio included) every 2 s;
- `RACKFORGE_AUDIO_CPUS` and `RACKFORGE_RENDER_CPUS` place the audio thread
  and the workers, for D2.

Not done: the native control stacks (their size guards against stack
overflow in native control calls, which would take the engine down; left
for the memory budgets of D5), moving heavy control commands off the audio
thread, and the priority inversion on native control calls.

**Result** (same conditions as the baseline, three runs):

| Instrument | Stable voices, nothing late | Without a dropout | Overruns below the edge |
| --- | --- | --- | --- |
| Concert Grand | 12, 12, 12 (was 12, 12, 6) | 24 (24) | 0, 0, 0 (0, 0, 1) |
| RF-Musette Student 72 | 8, 8, 8 (was 8, 6, 6) | 10 (10) | 0, 0, 0 (0, 15, 21) |
| RF-5 Slow Strings | 24, 2, 24 (was 24 ×3) | 24 (24) | 0, 1, 0 (0, 0, 0) |
| RF-Organ Straight 888 | 24 (24) | 24 (24) | 0 (0) |

- Threads 32 → 18; realtime threads 5 → 4, none unexpected.
- RSS 404 → 374 MB.
- Past the edge nothing moved, as expected: RF-Musette at 10 voices still
  16 overruns per run, at 12 voices 3619–3621.
- Boot to the first period: median 10.4 → 10.2 s, worst 11.4 → 10.4 s.

- **RSS: NOT MET.** It fell 30 MB, not ≥ 128 MB, because the control stacks
  were not touched.
- **Realtime threads: met.**
- **p99 and xruns: met.** Stray overruns below an instrument's edge went
  from 3 in 12 instrument runs to 1, and that one is not a stray: RF-5 at
  4 voices peaked at 100.7 % of the period (5336 µs against 5333). RF-5
  reaches 86–94 % there in every run, so it is that program's own margin
  running out, not a spike.

### D2. CPU for audio

Topology-aware placement, chosen by the profile and the board: the USB
controller's interrupt, the network and storage away from the core that
renders the heaviest work; workers pinned or not by measurement; the
worker count from the board; the `performance` governor applied and
verified; RT throttling kept as a safety net unless measurement says
otherwise.

*Prediction:* p99 and max block time fall measurably at the same polyphony,
and stable polyphony rises.

**Facts of the Pi 4 that bound the choices:** the USB controller sits behind
PCIe, and its MSI cannot leave CPU 0 (writing its affinity is refused); the
SD card and the SDIO Wi-Fi share one interrupt, and that one and Ethernet's
can move. RF-Musette does not render in units, so all of it runs on the
audio thread; Concert Grand (4 units) and RF-5 (5 units) spread over the
workers.

**Exploration** (edge plan: Concert Grand 8–24 voices, RF-Musette 6–12,
20 s a step, two runs each; overruns/underruns and the peak per step):

| Placement | Concert Grand 16 / 20 / 24 voices | RF-Musette 8 / 10 / 12 voices |
| --- | --- | --- |
| scheduler, `ondemand` | 1/0, 1/0, 1/0 at 116–142 % | 0–5/0, 16–18/0, 3583/80 |
| scheduler, `performance` | 0–1/0, 0–1/0, 1/0 at 97–149 % | 0/0, 16/0, 3620/64 |
| audio on 3, workers on 0, 1, 2 | 4/0, 37–46/0, 57–61/2–3 at 162–280 % | 0/0, 16/0, 3612/68 |
| audio on 3, workers on 1, 2, 3 | 4–6/0, 31–52/0–1, 62–70/2 at 179–275 % | 0–12/0, 16/0, 3600/73 |
| audio on 0 with the USB interrupt, workers on 1, 2, 3, storage and network interrupts on 1 | 5–6/0, 41–49/0, 63–65/2–3 at 150–264 % | 0–12/0, 16/0, 3611/92 |

- `performance` helps a little: RF-Musette at 12 voices drops ~20 % fewer
  periods (64–66 underruns against 80), and the stray overruns at 6 and 8
  voices went.
- Pinning hurts Concert Grand badly wherever the threads go: twice the
  peak, dozens of overruns, audible dropouts past 20 voices. The
  scheduler, free to move a worker off a busy core, does better than any
  fixed placement tried.
- Nothing moves RF-Musette: its audio thread alone on a core clear of the
  interrupts plays exactly as it does anywhere else.

A second round separated the causes, all with `performance`:

| Placement | Concert Grand 16 / 20 / 24 voices | RF-Musette 8 / 10 / 12 voices |
| --- | --- | --- |
| audio on 3, workers free | 4–9/0, 32–49/0–1, 62–72/2–3 at 163–275 % | 0–1/0, 16–17/0, 3641/56 |
| 2 workers | 1/0, 1–2/0, 1/0 at 102–157 % | 0–11/0, 16/0, 3626/63–92 |
| 4 workers | 0/0, 0/0, 0–1/0 at 58–104 % | 0–9/0, 16/0, 3611/82–92 |

- Pinning the audio thread alone is enough to do the damage: a worker the
  scheduler puts on its core waits behind it, the audio thread being the
  higher priority.
- Four workers, one per core, give Concert Grand's four units a core each:
  its peak falls from 97–149 % to 58–104 % and nothing is late up to 24
  voices. The engine's default keeps a core back (CPUs − 1), which is right
  where something else needs it; on a dedicated Pi 4 nothing does.

**Chosen for the Pi 4:** `performance`, four workers, no affinity, the
interrupts where they are. Applied through `rackforge-pi apply cpu`.

**Result** (full plan, three runs, against D1):

| Instrument | Stable voices, nothing late | Overruns, whole ramp | Peak at 16+ voices |
| --- | --- | --- | --- |
| Concert Grand | 24, 20, 24 (was 12, 12, 12) | 0, 1, 0 (20, 4, 5) | 80–102 % (123–133 %) |
| RF-Musette Student 72 | 8, 8, 6 (8, 8, 8) | unchanged | unchanged |
| RF-5 Slow Strings | 24, 24, 24 (24, 2, 24) | 0, 0, 0 (0, 1, 0) | 63 % (91 %) |
| RF-Organ Straight 888 | 24 (24) | 0 (0) | 49 % (72 %) |

Dropouts: none at any step for the three instruments that render in units,
as before; RF-Musette's begin at 12 voices, as before. 49 °C, never
throttled.

**Met** for every instrument that renders in units: lower peaks and more
stable voices. **Not met** for RF-Musette, which renders on the audio
thread alone: no placement or governor moves it, and the 6 is a run in
which 8 voices, which sit at 97–100 % of the period, went over once.

### D3. Services and maintenance

Inventory, then a profile: keep audio, MIDI, controllers, the interface, SSH
and NetworkManager; disable what the profile does not use (Bluetooth,
Avahi if the interface is reached by address, udisks2, rpi-connect, cron
jobs, man-db, e2scrub); move apt, fstrim and eeprom checks to an explicit
**maintenance** state that never runs while the engine is playing.

*Prediction:* time to ready falls by at least 1 s; no background job starts
during a two-hour soak.

**Where a boot's time went** (Pi 4, before D3, from the kernel's start):
userspace at 2.0 s; udev announced the boot partition at 4.1 s, its fsck
took 0.8 s and its mount ended at 5.0 s; only then did `sysinit.target`
let the engine start, at 5.5 s. The engine needs none of that partition.
Inside the engine (the new `STARTUP_STEP` lines), its 4.7 s went to opening
the plugin packages (1.65 s), loading them (0.7 s), instances (0.5 s), MIDI
and audio (0.5 s), the control server (0.8 s) and locking memory (0.5 s).
The 1.65 s was the engine decoding every plugin's banner, icon and splash
PNG (some 17 MB from the card) on every start, which the store had already
validated when it installed them and which the engine never shows. The
control server's 0.8 s was seeding and reading every controller's factory
maps before it opened its socket. The bootloader's configuration (boot
order SD first, no network or UART waits) offers nothing to gain.

**Built:**

- The engine's unit no longer names `local-fs.target` and `sound.target`
  (its default dependencies already order it after the filesystems, so a
  normal install is unchanged); the dedicated profile's drop-in turns the
  default dependencies off and orders it after the journal and udev alone,
  with a private `/tmp` of its own (`PrivateTmp=disconnected`). The governor
  unit is ordered after nothing.
- The engine opens store-installed packages without decoding their
  branding again; a package dropped into `plugins/` by hand is still fully
  validated.
- The control server binds its socket at once and builds the controller
  maps on its own thread; a client that connects meanwhile is queued.
- Services: scheduled maintenance (apt, man-db, e2scrub, fstrim, the dpkg
  backup) only through `rackforge-pi maintenance`; no bootloader check or
  e2scrub snapshot search at boot; no udisks2, no wait-online; Bluetooth
  only with a paired device.

**Result so far** (10 boots each, median and worst, seconds from the
kernel's start):

| | Engine started | First period | Web | SSH | Network |
| --- | --- | --- | --- | --- | --- |
| D1 | 6.0 | 10.2 (10.4) | 16.9 | 17.0 | 20.8 |
| D2, CPU module only (5 boots) | 6.2 | 10.3 (10.4) | 17.1 | 17.2 | 20.2 |
| D3, services and early start | 4.2 | 9.0 (9.25) | 15.8 | 15.9 | 18.9 |

Starting 1.9 s earlier made the engine ready only 1.3 s earlier: it now
starts while the rest of the system is still reading the card, and its own
start grew from 4.2 to 4.8 s. On a Pi 4 the engine's start is bound by the
card.

With the branding and control-server changes in the engine (10 boots):

| | Engine started | First period | Web | SSH | Network |
| --- | --- | --- | --- | --- | --- |
| D3 with the engine changes | 4.2 | **7.5 (7.6)** | 14.4 | 14.5 | 17.5 |

2.7 s earlier than D1 from the kernel's start, and everything else more
than 2 s earlier. The user measured about 20 s from power to sound before
any of this; that puts the firmware, the bootloader and loading the kernel
at about 9.5 s, so power to sound is now about 17 s.

**The governor did not survive a reboot.** `verify` after the boots above
found every core on ondemand: those boots ran with it. Two things set it
after the unit: the CPU frequency driver registers late and starts on the
kernel's default governor, and raspberrypi-sys-mods ships a udev rule that
writes ondemand to every CPU as it appears. The CPU module now sets
`cpufreq.default_governor` on the kernel command line and replaces the
package's rule with one of the same name in `/etc/udev/rules.d`; after a
reboot every core reads `performance` and `verify` passes. A marker saying
"applied" would have reported success throughout.

**Soak** (`rackforge-pi-bench soak`, 120 minutes, a chord every 5): no unit
started on its own; no overrun, underrun or engine restart; peak 60 % of
the period; 46–50 °C, never throttled; `performance` on every core
throughout.

**Result: met.** Time to the first period fell 2.7 s (the prediction asked
for 1 s), and nothing started in the background during the soak.

**The initramfs.** The firmware read an 11.8 MB initramfs beside the
10.2 MB kernel; the initramfs unpacked, ran its udev, checked the root and
mounted it at 1.81 s. The kernel builds in what this root needs (ext4 and
the SD card's MMC block driver), and without an initramfs it mounts the
root read-only by itself, at 1.16 s, after which `systemd-fsck-root` checks
it ("rootfs: clean") before it is remounted read-write: the check is kept.
Five boots each, through `tryboot`:

| | First period, from the kernel | Before the kernel (estimate) |
| --- | --- | --- |
| with the initramfs, `performance` from boot | 7.34 (7.55) | baseline |
| without | 6.78 (6.81) | 1.2 s less |

About 1.8 s less from power to sound; with the user's 20 s from before,
about 15 s now.

A boot change decides whether the Pi boots at all, so the boot module
never writes config.txt straight away. `apply` stages the change in
`tryboot.txt` and `verify` reports it as pending; `rackforge-pi try-boot`
restarts once from that file; `rackforge-boot-trial.service` checks, on that
boot alone, that the engine played its first period, records the proof
(the lines, the kernel, the board) and runs `apply boot`, which now moves
the lines into config.txt and puts back what only the trial needed. A trial
that fails or hangs is undone by unplugging the Pi. Once kept, the change
stays across kernel updates: Raspberry Pi kernels have always built ext4
and the SD card's driver in, and `rackforge-pi revert boot` brings the
initramfs back at any time. Done on the Pi: the trial passed, config.txt carries
`auto_initramfs=0`, and nothing of the trial is left.

**D3 at its end** (5 ordinary boots after the trial; no initramfs loaded;
`verify` passes):

| | First period | Web | SSH | Network | Reboot, SSH gone to SSH back |
| --- | --- | --- | --- | --- | --- |
| D1 | 10.2 | 16.9 | 17.0 | 20.8 | 33.6 |
| D3 | **6.8** (6.9) | 13.8 | 13.9 | 16.9 | 27.2 |

The instrument sounds 3.4 s sooner after the kernel starts and a whole
reboot is 6.4 s shorter. From power, about 15 s against the user's 20 s.

### D4. Storage

Volatile, size-capped journal; telemetry not written to the card every
second; `/tmp` in RAM; then the evaluation of a read-only base system with
sessions, configuration, plugins and data on a persistent writable area,
and an update path that works with it.

*Prediction:* card writes during an hour of playing fall to near zero.

### D5. Profiles by hardware

Pi 4 and Pi 5, RAM sizes, OS releases, peripherals in use. Budgets
(workers, memory) derived from the board.

### D6. The kernel, only if it earns it

Build the Raspberry Pi kernel of the installed series with `PREEMPT_RT`,
install it beside the stock one, boot it once through the firmware's
`tryboot`, verify after the reboot (engine ready, audio, MIDI, network), and
make it the default only then; the stock kernel stays as the fallback.

*Prediction:* worth installing only if `cyclictest` max latency under load
falls clearly **and** the engine bench shows fewer xruns or higher stable
polyphony. Otherwise it stays out, and this section says so.
