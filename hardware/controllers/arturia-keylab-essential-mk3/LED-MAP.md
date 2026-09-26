# KeyLab Essential mk3 RGB LED map

Read off the hardware, not from a datasheet: each ID was lit on its own with
`cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example
led_sweep` and the control that lit was written down. Rerun that example if a
firmware revision moves anything.

The IDs are the `control_id` argument of `protocol::rgb_led_message`, sent as
`F0 00 20 6B 7F 42 04 01 16 <id> <r> <g> <b> F7`. Values are 7-bit; the
protocol clamps each channel to `0x7F`.

| ID | Control | | ID | Control |
|---|---|---|---|---|
| `0x00` | MIDI Ch | | `0x16` | Record |
| `0x01` | Bank | | `0x17` | TAP |
| `0x02` | Transp − | | `0x18` | **OLED Button 1** |
| `0x03` | Transp + | | `0x19` | **OLED Button 2** |
| `0x04` | Oct − | | `0x1A` | **OLED Button 3** |
| `0x05` | Oct + | | `0x1B` | **OLED Button 4** |
| `0x06` | Prog | | `0x1C` | Pad 1 |
| `0x07` | Part | | `0x1D` | Pad 2 |
| `0x08` | Arp | | `0x1E` | Pad 3 |
| `0x09` | Chord | | `0x1F` | Pad 4 |
| `0x0A` | Scale | | `0x20` | Pad 5 |
| `0x0B` | Hold | | `0x21` | Pad 6 |
| `0x0C` | Save | | `0x22` | Pad 7 |
| `0x0D` | Quant | | `0x23` | Pad 8 |
| `0x0E` | Undo | | `0x24` | — nothing lit |
| `0x0F` | Redo | | `0x25` | — nothing lit |
| `0x10` | Loop | | `0x26` | — nothing lit |
| `0x11` | ⏪ | | `0x27` | — nothing lit |
| `0x12` | ⏩ | | `0x28` | — nothing lit |
| `0x13` | Metronome | | `0x29` | — nothing lit |
| `0x14` | Stop | | `0x2A` | — nothing lit |
| `0x15` | Play | | `0x2B` | — nothing lit |

## What this settled

The four buttons under the screen sit at `0x18`–`0x1B`, exactly where
`protocol::button_led_message` puts them, and they **light when addressed on
their own**. The hypothesis that the IDs were wrong is refuted: both the IDs
and the hardware are fine, so a dark button is something the rest of the
sending does, not a bad address.

`RGB_LED_COUNT` is `0x2C`, which is eight IDs more than the device answers
to. `0x24`–`0x2B` accept the message and light nothing. That costs eight
redundant messages on every ambient repaint, at `LED_SETTLE_MS` apiece.
Harmless, but it is not a map of this device.

## Still open

The four OLED buttons are dark in normal use while every other LED takes the
ambient colour. Since the address is right, the suspect is the footer message
(`protocol::footer`), which is the only thing sent to those four and to
nothing else: it carries a frame byte per button that is `0x00` for both
`Normal` and `Disabled`. If the device reads frame `0x00` as "no button here"
and blanks the LED, it would explain why exactly these four go dark and
nothing else does. Untested — it needs the hardware.
