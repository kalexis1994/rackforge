# Arturia MiniLab 37 — sources

The package describes the MiniLab 37 in its **ARTURIA program**. No value
was measured on hardware.

Arturia's MiniLab 37 manual says it is the MiniLab 3 with 37 keys and its
pads in two rows. It gives the same knob, fader and pad numbers, and names
its two ports. Bitwig's MiniLab 37 extension gives the port names on each
system. The knob mode and the pedal are documented for the MiniLab 3 only
(`../arturia-minilab-3/SOURCES.md`, [ML3]).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UM | MiniLab 37 User Manual 1.0 (2026-05-28) | https://dl.arturia.net/products/minilab-3/manual/minilab-3_37_Manual_1_0_0_EN.pdf | 2026-09-25 | `53ce003e6d854eafd226ae2e67a6018d634104b5f1aa6038d460761f2167494a` |
| BW | Bitwig, `MiniLab37ExtensionDefinition.java` and the shared `MiniLab3Extension.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/arturia/minilab3 (commit `3379f3d4`) | 2026-09-25 | `ed742049…31324de` (definition), `bbe692e3…35d903` (extension) |
| ML3 | The MiniLab 3 package's sources: its MIDI Control Center manual (MCC) | `../arturia-minilab-3/SOURCES.md` | 2026-09-25 | — |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The MiniLab 37 and the MiniLab 3 | the same but for the keys (37) and the pads (two rows of four) | Documented | UM 1.1 |
| Ports | `MiniLab37 MIDI` and `MiniLab37 DAW` | Documented | UM 5.2 "The MIDI port names are MiniLab37 MIDI and MiniLab37 DAW" |
| Port names | Windows and Mac `Minilab37 MIDI`, `Minilab37 DAW` (`N- …` for more units); Linux `Minilab37 Minilab37 MIDI`, `Minilab37 Minilab37 DAW` | Official software | BW `listAutoDetectionMidiPortNames` |
| What each port carries | the keys and all playing on MIDI; the DAW program's controls on DAW | Official software | BW: the note input on port 1 (MIDI) takes every message; knobs, faders and transport are read on port 0 (DAW) |
| Knobs | CC 74, 71, 76, 77, 93, 18, 19, 16, keyboard channel | Documented | UM 5.4.1–5.4.2 |
| Knob mode | absolute | Documented, for the MiniLab 3 | ML3 (MCC 3.6) |
| Faders | CC 82, 83, 85, 17, keyboard channel | Documented | UM 5.4.3 |
| Pads | notes on channel 10: bank A 36–43, bank B 44–51; pads 1–4 the far row, 5–8 the near one | Documented | UM 4.4, 5.4.4 |
| Touch strips | pitch bend; modulation CC 1 | Documented | UM 4.3 |
| Pedal | the Sustain pedal type sends CC 64 | Documented, for the MiniLab 3 | ML3 (MCC 4.4.1) |
| Transport (Shift + Pad 4–7) | **not declared** | — | UM 4.1 names the functions. Their CCs (105–108) are documented for the MiniLab 3; on the 37, Bitwig reads them on the DAW port in the DAW program, and no source gives their port in ARTURIA |
| Slots and roles | as the MiniLab 3: faders `control-1.1`–`1.4`, knobs `control-2`, pads by note, modulation `mod-wheel`; the eight-knobs-and-four-faders rule | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Which program the MiniLab 37 starts in.** In the DAW program the
   controls move to the DAW port.
2. **The transport's port in the ARTURIA program.** If it is the MIDI port,
   Play and Stop can take the transport as on the MiniLab 3.
