# Novation Launchkey 25, 49 and 61 [MK2] — sources

The package describes the Launchkey MK2 in **Extended (InControl) mode**,
which RackForge sets on connect. Its pots and sliders stay in InControl and
its drum pads go back to basic, so the pads keep playing. No value was
measured on hardware.

Every number is Novation's own, from the Programmer's Reference Guide.
Ableton's and Bitwig's scripts use the same mode and read the same numbers,
and they correct the guide on one point (Track < >).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launchkey MK2 Programmer's Reference Guide 1.01 | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/novation/downloads/10535/launchkey-mk2-programmers-reference-guide.pdf | 2026-09-25 | `0e56bb1e8a09b45953dc6969a0609391f80417d370354b113f5dd734cd7d3178` |
| AB | Ableton Live 12 MIDI Remote Script `Launchkey_MK2` (decompiled): `Launchkey_MK2.py`, `ControlElementUtils.py`, `consts.py`, `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Launchkey_MK2 (commit `0336151d`) | 2026-09-25 | `819dfd41…cf4df9`, `d1c5ac5c…b85a2f`, `9e53c65f…a48784`, `d71376a2…3149d5` |
| BW | Bitwig, `LaunchkeyMk2ControllerExtension.java`, `…ExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/novation/launchkey_mk2 (commit `ae6a9fa0`) | 2026-09-25 | `68a83bc2…81795d4`, `2f6090dd…cd64a561` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Ports | MIDI (port 1) and InControl (port 2); keys and wheels always on MIDI | Documented | PR p3–4 |
| Modes | Basic at connection; Extended by `9F 0C 7F` on the InControl port, answered with the same message; sections to InControl or basic with `9F 0D`/`0E`/`0F` 7F or 00 (pots, sliders, pads) | Documented; Official software ×2 | PR p4–5; BW sends `9F 0C 7F`, then `9F 0F/0E/0D 7F`; AB the InControl buttons 12–15 |
| In Extended mode | pots, sliders and buttons send on the InControl port, channel 16 | Documented (the port); Official software ×2 (channel 16) | PR p4; AB `STANDARD_CHANNEL = 15`; BW status 191 (`BF`) |
| Port names | Windows `Launchkey MIDI` / `MIDIIN2 (Launchkey MIDI)`, output `MIDIOUT2 (Launchkey MIDI)`; Linux `Launchkey MIDI MIDI 1` / `MIDI 2`; Mac "Launchkey MIDI" / "LaunchKey InControl" | Official software (Windows, Linux); Documented (Mac, as the guide names the ports) | BW `listAutoDetectionMidiPortNames` (its Mac entry names an "LK Mini"); PR p4 |
| Sizes | 25, 49, 61; the port names carry no size | Official software; Documented | BW; PR p12 (the Identity Reply's family member tells them apart: 00h, 01h, 02h) |
| USB ids, for the record | Novation 4661 (0x1235); Identity Reply `F0 7E 00 06 02 00 20 29 7A 00 FM1 00 …` | Official software; Documented | AB `controller_id`; PR p12 |
| Pots 1–8 | CC 21–28, absolute | Documented; Official software ×2 | PR p14; AB `make_encoder(range(21, 29))`, absolute |
| Sliders 1–8, master (49/61) | CC 41–48; master CC 7 | Documented; Official software | PR p14; AB `make_slider(range(41, 49))`, `Master_Slider` 7 |
| Slider buttons (49/61) | CC 51–58, the ninth CC 59; 127 / 0 | Documented; Official software | PR p15; AB `Mute_Button_n`, `Master_Button` |
| Track < > | CC 102 previous, 103 next | Official software ×2 | AB `Track_Left_Button` 102, `Track_Right_Button` 103; BW 102 `prevTrack`, 103 `nextTrack`. PR p15 lists Left 103, Right 102 — the scripts are taken over it |
| Transport | Rewind 112, Fast Forward 113, Stop 114, Play 115, Loop 116, Record 117; 127 on press | Documented; Official software ×2 | PR p15; AB; BW |
| InControl notes | 12–15 on channel 16: the mode's echo and the InControl buttons | Documented | PR p4–5. Declared so they never play |
| Drum pads | basic: notes 40–43/48–51 (top) and 36–39/44–47 (bottom) on the MIDI port; not declared | Documented | PR p6, p14. Left to play as a keyboard's pads |
| Held from instruments | every declared control (`plays = false`) | Convention | catalog README |
| Slots | sliders 1–8 `control-1`; pots `control-2`; Track `step`; Rewind/Fast Forward `step-2` | Convention | catalog README |
| Roles | the eight-knobs-and-nine-faders rule, the master slider the master level | Convention | catalog README |
| Transport actions | Play and Stop | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The Mac port names** exactly as CoreMIDI shows them, and the Linux ones
   under a kernel that names ports after the USB jacks.
2. **The 25:** one package serves the three sizes, so the 25's pots take the
   second row as on the 49 and 61, and its first row stays empty.
3. **The drum pads' channel** in basic mode, which the guide does not give.
