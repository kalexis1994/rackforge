# KeyLab recovery workspace

## Safety state

The keyboard currently runs restored official Arturia firmware 1.2.1 and
enumerates normally as:

- USB VID/PID: `1C75:028C`
- USB revision: `1201`
- USB serial: `7446400845190410`
- Windows services: `arturiausbmidi_sc` and `TUSBAUDIO_ENUM`

`stock-official-1.2.1/` contains a verified local copy of Arturia's official
recovery package and its extracted 61-key image. This is a **stock baseline**,
not yet a dump read back from this particular keyboard. It may not contain
device-specific configuration stored outside the application image.

## Verified stock files

| File | Bytes | SHA-256 |
|---|---:|---|
| `keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3` | 581,399 | `C57604BEBB688F3C03D508FF5A24171FB142331EDD64A1CDDE815AAD06DBFC93` |
| `keylab-essential-61-mk3.bin` | 193,412 | `819258D9EFE53E5E5026489F097E3E0DC9F132FD051EE00D28614000D2965269` |

## Official bootloader entry

Arturia documents the following recovery procedure for all KeyLab Essential mk3
sizes:

1. Disconnect USB.
2. Hold contextual buttons 3 and 4 below the display.
3. Connect USB while holding both buttons.
4. The display and LEDs should remain off.

The Arturia product description says that the updater keeps VID/PID
`1C75:028C`; detection must therefore use interface class/service rather than
looking for a different PID.

Disconnecting and reconnecting normally should return to the stock application,
provided no erase/download command was issued.

## Important limitation

The documented N32 BOOT command set exposes:

- chip/bootloader identification;
- option-byte reading;
- flash erase/download/CRC;
- reset and jump to USER1.

It does **not** document arbitrary flash upload or arbitrary SRAM download and
execution. Therefore:

- a bit-for-bit device backup probably requires accessible SWD;
- the official `.kle3` package is currently the only verified restore image;
- lowering read protection is forbidden because it may mass-erase flash;
- only bounded diagnostics may be attempted while the pinned official package
  continues to restore the application reliably.

## Offline display-hook candidate

`firmware/patch/` now produces a local candidate that adds a bounded
framebuffer SysEx hook without replacing the official application. All
historical and rebuilt artifacts remain explicitly rejected and non-installable.

Analysis of the 64-byte Arturia image header confirmed an additive 8-bit
checksum: the sum of all header bytes is zero modulo 256. A second additive
checksum at offset `0x0A` makes the sum of the complete payload zero modulo 256.
The rule was reproduced across three independent official images. No
cryptographic signature field is visible in the header.

The exact stock `.kle3` was reapplied successfully through MIDI Control Center
on 2026-07-29. The keyboard left bootloader revision `0.2.0`, booted firmware
`1.2.1`, and Windows recovered its normal MIDI interfaces. Three successful
stock restorations now validate the documented button 3+4 entry and
same-version recovery path without opening the enclosure.

This does not make a modified update risk-free: the updater or bootloader can
still reject its changed payload before writing it. The generated candidate
must continue to preserve the stock package, validate every bound offline and
avoid option-byte operations.

The first display-hook candidate was attempted once on 2026-07-29. MIDI
Control Center completed the transfer and then reported an error; the keyboard
remained in bootloader mode. The same pinned official 1.2.1 package restored it
successfully a second time. The rejected package is no longer emitted with an
installable `.kle3` extension by the default build.

## Rejected same-size inert diagnostic

The second diagnostic kept the official image length and 64-byte header exactly
unchanged. It replaces exactly 16 erased bytes at `0x08035CC0` with a
pre-rename non-executable marker and changes no vectors, instructions,
dispatcher bytes, or control flow.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| inert image | 193,412 | `63924E18E447FD719D5F7CE261A0976DA19A6A13DE41BB82ED2CB380178B33ED` |
| single-test `.kle3` | 581,399 | `3CDC8544844ADA908677342EA1DFD783DD43C7EBC6F3BC826917AFDEFC2E7176` |

The transfer completed, but the application did not enumerate and MIDI Control
Center reported: `Failed to open the device. Please verify the device is not in
use by another application.` Windows still exposed only the bootloader
interface; no competing DAW or RackForge bridge process was running. Reinstalling
the pinned official package restored both normal interfaces immediately.

The same-size diagnostic is rejected and must not be retried. Its result is
now explained by the payload checksum: it retained the official value `0xD9`,
but its modified payload required `0xCE`. It therefore never constituted a
valid no-code-change boot test. The first hook candidate had the same stale
field, so neither failed experiment provides evidence that the hook itself
crashed.

## Successful checksum-correct integrity probe

A third, non-executable probe changed a 16-byte marker in the same erased gap
and refreshed only the payload and header checksums. MIDI Control Center
installed it successfully, the application booted normally, and Windows
reported both the base MIDI and `MEDIA` interfaces as `OK`.

The one-time `.kle3` was immediately renamed to a non-installable archived
extension. The official package remains unchanged and pinned for recovery.

## Ineffective display-hook candidate

The checksum-correct display hook installed and booted, but a complete bounded
framebuffer upload produced no pixel change and the official `DAW Program` ACK
disappeared. Static review confirmed that its hook address processes an
internal UI structure rather than raw/general SysEx.

The package was archived under a non-installable extension and must not be
reused. The Raspberry display service was stopped and disabled so it cannot
send acquisition pulses during recovery. Restore the pinned official package
before further offline dispatcher analysis.

## Rejected raw SysEx callback hook

The next checksum-correct candidate hooked `0x0800B802` and booted normally.
Linux enumerated all four KeyLab ALSA ports, but neither the stock `DAW Program`
ACK nor any played MIDI note reached the host. This proves that the current
wrapper disrupts the primary MIDI path.

The official 1.2.1 package restored the keyboard successfully. The candidate
SHA-256
`00ce83922290bf11b69d872b5aa4e54a175dcd19f5772e86f40810a920446e29`
is rejected, archive-only and must not be installed again. The display service
remains disabled while RackForge continues with the official firmware.

`watch_bootloader.ps1` only enumerates Windows devices. It contains no USB
transfer, DFU request, erase, download, reset, or driver-changing operation.
