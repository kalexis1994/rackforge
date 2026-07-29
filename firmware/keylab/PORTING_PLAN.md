# artupy native DOOM port plan

## Current conclusion

The processor is fast enough to make a reduced Doom renderer plausible, but the
stock memory budget is not. The first engineering problem is storage and RAM,
not Rust or instruction throughput.

| Resource | KeyLab inferred | RP2040 Doom reference | Gap |
|---|---:|---:|---:|
| CPU | Cortex-M4F, up to 144 MHz | 2× Cortex-M0+, overclocked to 270 MHz | plausible with reduced output |
| SRAM | 144 KiB | 264 KiB | -120 KiB |
| Internal flash | 256 KiB total | 2 MiB external flash | -1.75 MiB |
| Application slot | 226 KiB | code/static data near 256 KiB | already smaller |
| Shareware WAD | 4.00 MiB | transformed/compressed into external flash | cannot fit internally |

## Phase 0 — no electrical contact

1. Read the exact MCU marking and package from a sharp board photograph.
2. Identify every memory IC, especially 8-pin SPI/QSPI flash or PSRAM.
3. Identify the LCD module/controller and trace its board connector.
4. Identify unpopulated pads labelled SWDIO, SWCLK, NRST, BOOT, RX/TX, 3V3,
   or GND.
5. Map which controls and keybed boards are connected to the main MCU.

Deliverable: annotated board map. No powered-open measurements.

## Phase 1 — recoverability before replacement firmware

1. Document the official updater's USB enumeration and package validation.
2. Confirm whether the 30 KiB boot region belongs to Arturia, the N32 ROM
   bootloader handoff, or both.
3. Determine whether a same-version official recovery is accepted.
4. Confirm SWD availability without changing option bytes.
5. If debug readout is permitted, back up the entire flash twice and compare
   cryptographic hashes.

Do not lower read protection: the N32G455 documentation says this can mass-erase
main flash. Do not probe a powered board until ground and 3.3 V are identified.

## Phase 2 — temporary native execution

The safest first native experiment is not Doom and not a flash replacement. It
is a tiny program placed in SRAM through an already-proven debug/boot path:

1. preserve clocks, USB state, and flash;
2. toggle no pins;
3. return a known value through the debugger;
4. reset into untouched Arturia flash.

Only after that works should the program initialize one known peripheral, then
the LCD, then one button/encoder input. A Rust hardware abstraction layer can be
built incrementally from those observations.

## Phase 3 — Doom architecture

Preferred design if external memory can be fitted or already exists:

- keep a recovery-capable boot stub in the Arturia application slot;
- execute compressed code/assets from QSPI XIP where practical;
- use PSRAM for the reduced zone allocator and frame data;
- render directly into a small LCD tile/line buffer, never a full 320×200
  framebuffer;
- use keys/pads/encoders as controls;
- start without music, sound, saves, networking, or menu artwork;
- preprocess `DOOM1.WAD` on the PC into a compact, indexed format;
- restore features only after every shareware map survives memory stress tests.

Fallback if the board exposes no practical external-memory pins:

- stream preprocessed lumps over USB;
- keep game logic and renderer on the MCU;
- cache only the current line/tile and the smallest active level structures;
- accept that 144 KiB may still require deeper structural changes than
  RP2040 Doom.

## Stop conditions

Stop and restore the stock unit if any of these is unknown:

- exact supply voltage or pad identity;
- verified stock-firmware backup;
- bootloader/recovery entry method;
- read-protection consequences;
- LCD voltage and bus ownership;
- watchdog and clock configuration.
