# Startup availability policy

Status: implemented host phases; schema-free operational contract

RackForge optimizes for the first playable note, not for the moment every OS
service and management screen has finished loading. Every host follows the
same monotonic availability order:

1. `audio_ready`: the restored PLAY/LIVE graph has written or can render a
   real audio period through the selected output;
2. `control_ready`: musical input and host-owned controller surfaces are
   available;
3. `background_ready`: non-critical discovery and management work has been
   released.

A platform may omit an inapplicable controller phase, but it cannot publish a
phase and later move backwards. Every generation emits:

```text
STARTUP_PHASE host=<host> phase=<phase> elapsed_ms=<milliseconds>
```

## Scheduling is not arbitrary sleeping

Independent UI painting may overlap the critical path. It must not be awaited
by the engine and must not receive greater boot CPU or I/O priority. Fixed
delays are not readiness checks and are forbidden as ordering mechanisms.

The critical path contains only work required by the restored sound:

- resolve the selected output and required input;
- load the active plugin or LIVE graph;
- restore its preset, opaque state and live parameter overrides;
- allocate all real-time buffers;
- connect musical MIDI input;
- start the stream and complete its first period.

The following work is never allowed to revoke or delay a working audio path:

- full Plugin Manager health/catalog refresh;
- controller package upgrades and optional displays;
- Web server, network, store and release checks;
- inactive plugin UI/model preparation;
- maintenance and diagnostics.

If a controller or background phase fails, RackForge keeps audio alive and
reports the degraded layer. It may retry that layer independently.

## Platform realizations

| Host | Audio phase | Control phase | Background phase |
| --- | --- | --- | --- |
| Raspberry Pi / systemd Linux | Core notifies systemd after the first successful ALSA period | platform host and `.rfcontroller` start after the notified audio unit | NetworkManager and Web start afterwards on the appliance profile |
| Desktop | `DesktopAudio` publishes one complete device/plugin/MIDI generation | `.rfcontroller` supervisor starts after that generation | management/UI refreshes remain asynchronous |
| Android | active portable plugin and AAudio stream start on the engine loader | controller packages, LIVE metadata, Android MIDI ports and LITTLE follow | UI/catalog persistence continues without blocking audio |
| Browser host | AudioWorklet boots after the browser's required user gesture | Web MIDI is requested after the worklet is audible | persistent-storage and management work is fire-and-forget |
| VST3 | the selected RackForge instrument activates during DAW setup | optional host/controller integration is outside DSP activation | editor and catalog models are lazy |

## Readiness semantics

Opening a device is not sufficient evidence of availability. On ALSA,
`READY_TO_PLAY` and the systemd `READY=1` notification are emitted only after
`writei` completes one period. Desktop and Android publish `audio_ready` only
after their stream constructors return a complete generation.

Startup timeouts bound a damaged plugin or driver. A timeout may degrade or
restart that layer; it must not turn network availability into a permanent
dependency of audio, nor turn an optional controller into a prerequisite for
sound.

On appliance Linux, controller service readiness means that initial discovery
and every first driver launch attempt completed. It deliberately does not wait
forever for a physical USB device or display acknowledgement: an unplugged
keyboard must not make networking unavailable. Network work may overlap the
last device-specific handshake, but the controller unit retains a much higher
startup CPU and I/O weight during that overlap.

## Performance budgets

Measurements are split so regressions name their owner:

- power-on to kernel handoff (firmware/bootloader);
- kernel handoff to `audio_ready`;
- host process start to `audio_ready`;
- `audio_ready` to controller acquisition;
- controller acquisition to background/network readiness.

CI tests the monotonic phase contract. Hardware qualification records the
actual milestones because virtual builds cannot prove USB enumeration, device
driver or first-period latency.

### Raspberry Pi reference measurement

On the reference Raspberry Pi boot measured on 2026-08-26, systemd reported:

- Core process start at 5.765 s and `audio_ready` at 7.030 s;
- controller host readiness at 7.441 s;
- Arturia LITTLE OLED acknowledgement at 8.103 s;
- Web host start at 13.361 s, after NetworkManager.

The earlier profile reached audio at approximately 8.194 s and LITTLE at
9.075 s. The availability policy therefore moved first audio about 1.16 s and
LITTLE about 0.97 s earlier without placing network services on the audio
critical path. Firmware time precedes systemd's monotonic clock and must be
added separately when measuring physical power-button-to-sound latency.
