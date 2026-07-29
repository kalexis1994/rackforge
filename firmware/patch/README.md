# RackForge display hook

This directory builds an **offline firmware candidate** for the exact official
KeyLab Essential mk3 firmware image `1.2.1`.

## Hardware result

The first candidate was transferred on 2026-07-29 but MIDI Control Center
reported an error at the end and the keyboard remained in bootloader mode.
Reinstalling the pinned official package immediately restored normal firmware
1.2.1 and all MIDI interfaces.

That candidate is rejected and must not be installed again. The failure does
not represent a valid integrity baseline: the original generator refreshed the
header checksum but did not refresh the payload checksum at offset `0x0A`.
That historical build can no longer produce an installable `.kle3`.

The rebuilt candidate refreshes both checksums and has SHA-256
`1566c0f9a6aa491d4251cea3f4955a4112de68a4505e7130642383adc7c937df`.
It is a distinct artifact from the rejected first attempt. It was installed
once, booted normally, but did not react to a checksum-correct framebuffer
upload and suppressed the official `DAW Program` ACK used by the bridge.

Static review then corrected an earlier assumption: the internal structure
handled at `0x0802C31C` begins its command data at `+0x0E`, not `+0x0D`, and
this routine is not a raw/general SysEx dispatcher. The hook is therefore
ineffective and must not be reinstalled. The explicit
`-BuildArchivedInstalledPackageForAnalysis` switch accepts only its pinned hash
and can emit only a non-installable archive.

## Inert diagnostic

`build-inert-diagnostic.ps1` now creates a same-size diagnostic that changes
only a 19-byte `RACKFORGE-DIAG-v1` marker inside the verified erased-flash gap
`0x08035C0C..0x08035D97`. Its header is byte-identical to stock firmware and it
does not change vectors, executable instructions, the dispatcher, or control
flow. This renamed variant has not been installed on hardware. Its modified
payload requires checksum `0x1C`, while its byte-identical header deliberately
retains the stock value `0xD9`.

The pre-rename 16-byte diagnostic completed its transfer, but the normal
application did not enumerate and MIDI Control Center reported that it could
not open the device. The pinned official package restored normal firmware
immediately. That historical diagnostic required checksum `0xCE` but retained
`0xD9`; it is rejected and must not be installed again.

The same rule holds independently for both MiniLab 3 images available in MIDI
Control Center (`0xEF -> 0x11` and `0x1A -> 0xE6`). Offset `0x0B` remains
`0x01`; its exact semantics are still unknown.

## Checksum-correct integrity probe

`build-integrity-probe.ps1` creates the RackForge form of the minimal integrity
experiment. It changes a 19-byte marker in the same erased gap and refreshes
only the payload checksum at `0x0A` and its dependent header checksum at
`0x3F`. Its complete diff is therefore 21 bytes, with no executable or
control-flow change. This renamed variant is offline-validated but has not been
installed.

The pre-rename 16-byte/18-byte probe was installed once on 2026-07-29. MIDI
Control Center completed normally, the application booted, and Windows
recovered both normal MIDI interfaces. Its historical hash remains pinned for
archive validation and cannot be claimed by the renamed RackForge candidate.

The output deliberately ends in `.bin.offline-not-tested`. The explicit
`-BuildArchivedPackageForAnalysis` switch can only create a non-installable
archive when the candidate matches its pinned SHA-256 exactly. The script
cannot access USB.

The non-installable image and its manifest are built with:

```powershell
.\firmware\patch\build-inert-diagnostic.ps1
```

For reproducibility, the explicit `-BuildRejectedPackageForAnalysis` switch
creates only a disabled artifact ending in
`.kle3.rejected-do-not-flash`.

It does not contain a DFU client and the generated image must not be installed
until the official recovery path has been exercised successfully.

## Raw SysEx callback candidate

The current offline candidate changes four bytes at `0x0800B802`, the completed
raw SysEx callback for the primary MIDI endpoint, to branch into code appended
at `0x08036B80`. The parser supplies the stripped manufacturer payload at
`message + 0x0D` and its length at `message + 0x07`.

An assembly prefilter compares `7D 41 50 01`. Every other message stays outside
Rust: the wrapper restores all scratch registers, replays the displaced
`push {r4, lr}`, invokes the exact official delegate at `0x0800B7ED`, and
resumes at `0x0800B809`. The build fails if either fixed target is absent from
the final disassembly or if the previously observed bad relative branch
reappears.

RackForge messages use the existing manufacturer SysEx envelope:

```text
F0 00 20 6B 7F 42 7D 41 50 01 <operation> ... F7
                  |  A  P  v1
```

The hook supports:

- `01 FRAME_CHUNK`: copies up to 40 nibble-encoded bytes into the internal
  1024-byte framebuffer;
- `02 PRESENT`: calculates CRC-16/CCITT-FALSE over all 1024 bytes and calls the
  original LCD flush routine only when it matches the host value.

The first implementation deliberately keeps no persistent state and adds no
flash-writing command. During an upload the stock UI must be quiescent. A
concurrent renderer write causes the final CRC to fail instead of presenting a
mixed frame.

The candidate image is pinned as
`00ce83922290bf11b69d872b5aa4e54a175dcd19f5772e86f40810a920446e29`.
It was installed once: the application booted and exposed its ALSA ports, but
the official `DAW Program` ACK and every played MIDI note disappeared. The
official 1.2.1 firmware restored normal operation. This hook is rejected and
must not be retried.

No installable package can be emitted for it. The optional
`-BuildArchivedRejectedRawSysexPackageForAnalysis` switch only creates an
artifact ending in `.archived`, pinned for offline analysis.

## Build

From the repository root:

```powershell
.\firmware\patch\build-candidate.ps1
```

Generated files live under the ignored `firmware/tmp/display-hook/` directory.
Both image and package artifacts end in `.rejected-do-not-flash`; the default
build emits no file with an installable `.bin` or `.kle3` extension.
The build script:

1. compiles the Thumb hook at fixed addresses;
2. extracts the four-byte branch and appended code separately;
3. verifies that the input is the exact stock 1.2.1 image;
4. verifies the original bytes at the hook site;
5. writes a candidate `.bin` and a JSON audit manifest;
6. refreshes both the payload checksum at `0x0A` and the dependent header
   checksum at `0x3F`;
7. re-parses the result and checks all declared bounds and checksums;
8. validates the raw callback, official delegate and resume targets in the
   final Thumb disassembly;
9. only with `-BuildRejectedPackageForAnalysis`, creates a disabled artifact
   ending in `.rejected-do-not-flash` by replacing only the 61-key entry in the
   pinned official package.

It never opens a MIDI or USB device.

`protocol.py` is the matching transport-independent host encoder. A complete
frame uses 26 bounded `FRAME_CHUNK` messages followed by one `PRESENT`. Every
message fits the firmware parser's 100-byte payload buffer.
