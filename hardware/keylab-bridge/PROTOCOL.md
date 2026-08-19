# KeyLab Essential mk3 protocol

Technical record of the MIDI/SysEx protocol used by RackForge with the official
Arturia KeyLab Essential 61 mk3 firmware.

Evidence labels:

- **Hardware confirmed:** observed on the project's physical keyboard.
- **Public-software confirmed:** implemented by public integrations but not yet
  reproduced locally.
- **Hypothesis:** requires a bounded test or further reverse engineering.

## Reference hardware

- Arturia KeyLab Essential 61 mk3.
- Official firmware 1.2.1.
- Main endpoint whose name ends in `MIDI`.
- Initially captured over ALSA MIDI on Raspberry Pi 4B; the same messages are
  now used by Windows and Android transports.

These operations belong to the official DAW integration protocol. RackForge
does not update firmware, write templates, or modify user memories.

## Public sources

The protocol was cross-checked against public Bitwig, Ableton, FL Studio, and
Loopy Pro integrations and an unofficial KeyLab Essential mk3 programming
guide. Public behavior is not marked hardware-confirmed until reproduced on the
project keyboard.

## SysEx envelope

Every command uses:

```text
F0 00 20 6B 7F 42 <PAYLOAD> F7
```

Payload bytes must be 7-bit MIDI data (`00..7F`).

### DAW session

```text
CONNECT     02 0F 40 5A 01
DISCONNECT  02 0F 40 5A 00
DAW PROGRAM 21 11 40 02 00 01
```

The keyboard echoes the DAW Program SysEx exactly. RackForge treats that echo
as the acquisition/heartbeat acknowledgement; a successful MIDI write alone
does not prove that the surface is healthy.

An ALSA endpoint may appear before the keyboard UI is ready. After reproducing
that boot race, the driver waits for five seconds of stable USB identity before
the first message and leaves two seconds between acquisition attempts.

## Display

### Clear

```text
04 01 60 61
```

### Header

```text
04 01 60 01 02 <ASCII, maximum 18> 00 00
```

### Two-line body

```text
04 01 60 12 01 <LINE 1> 00 02 <LINE 2> 00 00
```

Both lines accept at most 18 ASCII characters. Screen type `12` keeps the
native header and footer available.

Hardware inspection confirmed that line 1 uses the firmware's heavier primary
stroke and line 2 uses a thinner secondary stroke. Weight belongs to the native
region; case, characters, and RackForge styles do not change it.

```text
HEADER
BODY: LINE 1 (primary) + LINE 2 (secondary)
FOOTER
```

Use line 1 for the focused value and line 2 for context or neighboring choices.

### Official screen catalog

Arturia's generated `Displays.py` exposes:

| ID | Enum | Kind |
| ---: | --- | --- |
| `10` | `eFS_1Line` | text |
| `11` | `e1Line` | text |
| `12` | `e2Lines` | two-line text |
| `13` | `e2LinesScroll` | scrolling text |
| `14` | `eKnob` | knob widget |
| `15` | `eFader` | fader widget |
| `16` | `ePad` | pad widget |
| `17` | `ePopup` | popup |
| `18` | `eBlinkScreen` | blinking text |
| `19` | `eLeftIcon` | built-in icon and text |
| `1A` | `eTopIcon` | built-in icon and text |
| `1B` | `e1InlineIcon` | one inline icon |
| `1C` | `e2InlineIcon` | two inline icons |
| `1D` | `ePartScreen` | structured Part screen |
| `1E` | `eFramedText` | framed text |
| `1F` | `e2InlineBlink` | blinking inline variant |
| `20` | `eAutoComponent` | automatic control feedback |
| `21` | `eForceDefault` | restore default feedback |
| `60` | `eBlankScreen` | logical blank screen |
| `61` | `eWhiteScreen` | clear/white logical mode |
| `62` | `eBorderedScreen` | predefined border |

`ePartScreen` receives numbered fields, not pixels. No reviewed public API
contains a bitmap, image, canvas, or framebuffer upload operation.

### Pixel-control research

The display cannot show a host-supplied bitmap, and this is settled rather
than assumed: firmware 1.2.1 was disassembled to answer it.

- The panel is a monochrome 128x64 ST7565-family display, drawn from a
  1,024-byte framebuffer in RAM and flushed eight pages at a time.
- **Only the internal rasterizer writes that framebuffer.** The whole
  image contains exactly one instruction referencing it: the
  initialisation. Every other access goes through the display context, and
  the pointer is only ever aimed at an internal buffer.
- The only SysEx route to the display, command `04`, accumulates its body
  one byte at a time into a buffer **capped at 100 bytes**, then reads it
  as a widget descriptor. It cannot carry a 1,024-byte frame, and it never
  interprets bytes as pixels.
- Four undocumented top-level commands exist (`0A`, `0C`, `23`, `25`).
  All of them land in that same bounded descriptor path.
- The runtime USB descriptor exposes MIDI Streaming and DFU runtime, with
  no HID or proprietary bulk graphics endpoint.

So RackForge composes screens from the official widgets and glyphs, and
`eWhiteScreen` clears content rather than lighting every pixel.

Two attempts have now been made at pixel control. The first invented a
SysEx opcode no firmware implements; the second confirmed by disassembly
that no such opcode exists to find. **Do not add a framebuffer transport
here unless a real primitive is proven on hardware first.** Arbitrary
pixels would need a modified firmware, which is out of scope for RackForge
and lives in its own project.

### Contextual footer

```text
04 01 60 03 <BUTTON 1> <BUTTON 2> <BUTTON 3> <BUTTON 4>
```

The high nibble selects position (`10`, `20`, `30`, `40`). The low
nibble selects:

| Attribute | Button 1 ID | Data |
| --- | ---: | --- |
| State/frame | `10` | `<frame> 00` |
| Text | `11` | `<ASCII> 00` |
| Icon | `12` | `<icon_id> 00` |

Other buttons add `10`, `20`, or `30`. Text is limited to seven
characters.

Hardware-confirmed minimal footer:

```text
04 01 60 03
  11 4F 4B 00
  21 3C 00
  31 3E 00
  41 42 41 43 4B 00
```

This renders `OK`, `<`, `>`, and `BACK`.

Footer frame values:

| Value | Public name | Local evidence |
| ---: | --- | --- |
| `00` | `NONE` | neutral candidate |
| `01` | `BAR` | confirmed bottom line |
| `02` | `FRAME_SMALL` | not yet observed |
| `03` | `FRAME_FULL` | confirmed outline, not fill |

`FRAME_FULL` does not invert/fill the button on firmware 1.2.1. Do not map a
pressed state to it expecting inversion.

## Physical inputs

Hardware-confirmed messages on MIDI channel 1:

| Input | Press/turn | Release |
| --- | --- | --- |
| Button 1 | `B0 2C 7F` | `B0 2C 00` |
| Button 2 | `B0 2D 7F` | `B0 2D 00` |
| Button 3 | `B0 2E 7F` | `B0 2E 00` |
| Button 4 | `B0 2F 7F` | `B0 2F 00` |
| Encoder left | `B0 74 00..3F` | — |
| Encoder neutral | `B0 74 40` | — |
| Encoder right | `B0 74 41..7F` | — |
| Encoder press | `B0 75 7F` | `B0 75 00` |

Release events do not repeat actions, but they remove pressed visual feedback.

### RGB LEDs

CC 44–47 are input messages and do not drive the lights. RGB output uses:

```text
04 01 16 <button_id> <r> <g> <b>
```

Context button IDs are `18`, `19`, `1A`, and `1B`. RGB values are
`00..7F`. The command is effective only during an acquired DAW session.

The portable controller runtime applies dim blue `R=10, G=40, B=64` to RGB
IDs `00..2B`: mode buttons, transport, context controls, and all 16 pads. It
turns the range off before releasing DAW mode and restoring the Arturia program.

## RackForge navigation contract

```text
Button 1     Button 2     Button 3     Button 4
OK           <            >            BACK
```

- `OK` starts or confirms editing.
- `BACK` cancels an active edit, otherwise leaves the current page.
- `<` and `>` move focus or change the selected value.
- Encoder turn mirrors previous/next; encoder press mirrors OK.

Short and long presses are mutually exclusive. Long press begins at 650 ms.

- Long BACK returns to the active PLAY plugin or LIVE selection and restores
  the selected sound anchor.
- OK + BACK, started within 250 ms and held for 650 ms, returns HOME and performs
  a global stop. Core enters IDLE, cancels drafts/audition, destroys sound
  runtimes, and keeps the audio device open with silence.
- The emergency chord has priority and never also emits OK LONG or BACK LONG.
- Plugins cannot consume or block either host-owned escape route.

### Reserved master controls

In DAW Program, Fader 9 is MIDI channel 1 CC 113 and maps to
`master_level`. Encoder 9 is CC 104 and maps to `master_pan`.

The pan encoder is treated as relative movement from authoritative session
state to avoid jumps after reconnect. A virtual center detent keeps center easy
to find while preserving both ends of the range.

The latest value temporarily replaces the header for 1.5 seconds:
`MASTER VOL n%`, `MASTER PAN L n%`, `MASTER PAN R n%`, or
`MASTER PAN CENTER`. The driver sends typed commands and Core removes the
reserved CC before plugin routing.

### Reserved PART action

PART emits channel 1 CC 119: 127 on press and 0 on release. A short press opens
or toggles keyboard parts. Holding PART while pressing a note sets the split;
holding it for 1.5 seconds without a note clears the split. Core consumes the
gesture and split note before plugin routing.

## Safe next visual test

1. Confirm `NONE (00)` removes the idle footer bar.
2. Observe `FRAME_SMALL (02)` once.
3. Restore the known footer after every sample and on timeout.
4. Stop immediately if the display ignores a value, blinks, or loses layout.
