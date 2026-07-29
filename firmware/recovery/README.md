# KeyLab recovery workspace

## Safety state

No firmware has been erased or written. The keyboard still runs Arturia firmware
1.2.1 and enumerates as:

- USB VID/PID: `1C75:028C`
- USB revision: `1201`
- USB serial: `7446400845190410`
- Windows service: `arturiausbmidi`

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
- no modified image will be installed before SWD/recovery is physically mapped.

`watch_bootloader.ps1` only enumerates Windows devices. It contains no USB
transfer, DFU request, erase, download, reset, or driver-changing operation.
