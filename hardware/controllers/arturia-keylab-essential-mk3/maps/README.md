# KeyLab Essential mk3 map for the RackForge instruments

`rackforge-instruments.rfmap` maps the KeyLab onto RF - Concert Grand,
RF-Organ, RF-106, RF-5, RF-7 and RF-Tines.

It ships with RackForge. `rackforge-core` embeds it
(`controller_map_store::factory_maps`), and the Pi engine, the desktop app
and the Android app offer it on start:

- A player with no KeyLab map gets it.
- A player who kept it as it came gets each new version.
- A map the player made before, edited, or removed stays as they left it.

Every mapping is written out, even where it matches a semantic default. A
mapping on a parameter drops that parameter's default role, so the map
spells out the whole layout rather than lean on the defaults.

The same rules apply across the six instruments:

- **Knob 9 and fader 9 are not mapped.** They keep the master pan and the
  master level.
- **Faders set levels and shape:** drawbars, envelopes, cutoff, room.
- **Knobs set tone and colour.**
- **Pads bank 1 hold the switches played most.**
- **Pads bank 2 hold the second-level switches and sound variants.** A
  Cycle steps through a few chosen values; a Toggle goes to its first value
  and back.
- **⏪ and ⏩ step a selector down and up** (preset key, range, transpose).
- **Bank is not mapped.** If it is the button that switches the pads
  between banks on the keyboard, a mapping would fire on every bank change.
- **The mod wheel is left to instruments that handle CC 1 themselves.**
  Those are RF-Organ (Leslie speed), RF-106 (the bender lever's LFO), RF-5
  (wheel mod) and RF-7 (wheel range and target). Only RF - Concert Grand and
  RF-Tines, which ignore CC 1, map it.

Pads 1–4 send notes 40–43 and pads 5–8 send notes 36–39, on channel 11.
Bank 2 sends 48–51 and 44–47. A pad counts as pressed at any velocity. A
mapped pad does not also play its note.

## RF-Organ

| Control | Parameter | How |
|---|---|---|
| Faders 1–8 | Upper drawbars B 16' … 1 1/3' | direct |
| Knob 8 | Upper drawbar B 1' | direct |
| Knobs 1–7 | Expression, Preamp Drive, Key Click, Rotary Mix, Rotor Inertia, Bass Trim, AO-28 Tone | direct |
| Pad 1 / 2 | Rotary: Chorale ↔ Tremolo / Brake | toggle / set |
| Pads 3–6 | Percussion on, Harmonic 2nd/3rd, Volume Soft/Normal, Decay Slow/Fast | toggle |
| Pad 7 / 8 | Upper Vibrato on / Scanner V1…C3 | toggle / cycle |
| Pads b1–b4 | Preset key B, A#, F (Flutes 8' & 4'), A (Full Swell) | set |
| Pad b5 / b6 / b7 | Leslie off ↔ Chorale / Lower Vibrato / Mic Dynamic ↔ Condenser | toggle |
| Pad b8 | Tremolo while held, Chorale on release | hold |
| ⏪ ⏩ / Undo Redo | Preset key / Scanner mode, down and up | step |
| Mod wheel | Native: below half is Chorale, above half is Tremolo | — |

## RF - Concert Grand

| Control | Parameter | How |
|---|---|---|
| Faders 1–8 | Brightness, Dynamics, Decay, Unison, Ambience Level, Room Size, Mic Distance, Stereo Width | direct |
| Knobs 1–8 | Hammer Hard, Hammer Mass, Detune, Sympathy, Treble Life, Lid, Wall Hardness, Mic Pattern | direct |
| Pad 1 | Action Grand ↔ Upright | toggle |
| Pads 2–4 | Room 100 / 311 / 3000 / 15000 m³, Mic 0.66 / 2 / 6 m, Lid open / half / closed | cycle |
| Pads 5–7 | Action, Release, Pedal noise off ↔ default | toggle |
| Pad 8 | Stereo Width mono ↔ default | toggle |
| Pads b1, b2 | Brightness 0.3…0.75, Dynamics 0.25…0.7 | cycle |
| Pad b3 / b4 | Honky-tonk detune 0.9 ↔ 0.5 / Dry (no ambience) ↔ 0.5 | toggle |
| Pads b5–b8 | Wall Hardness, Mic Pattern, Decay, Sympathy | cycle |
| Mod wheel | Ambience Level | direct |

## RF-106

| Control | Parameter | How |
|---|---|---|
| Knobs 1–8 | PWM, Sub, Noise, VCF Env, VCF LFO, VCF Kybd, LFO Delay, VCA Level | direct |
| Faders 1–8 | Attack, Decay, Sustain, Release, Cutoff, Resonance, LFO Rate, DCO LFO | direct |
| Pads 1, 2 | Pulse, Saw | toggle |
| Pad 3 / 4 | Chorus I / Chorus II (press again for off) | toggle |
| Pads 5–7 | PWM LFO ↔ Manual, VCF Env + ↔ −, VCA Env ↔ Gate | toggle |
| Pad 8 | HPF 0…3 | cycle |
| Pad b1 | LFO trigger, the lever push, while held | trigger |
| Pad b2 / b3 | Portamento on / Assign Unison, Poly 1, Poly 2 | toggle / cycle |
| ⏪ ⏩ | Range 16' / 8' / 4' | step |
| Mod wheel | Native: DCO LFO depth, as the lever | — |

## RF-5

| Control | Parameter | How |
|---|---|---|
| Knobs 1–4 | Osc A PW, Osc B Fine, Osc B Frequency, Filter Env Amount | direct |
| Knobs 5–8 | Filter Attack, Decay, Sustain, Release | direct |
| Faders 1–8 | Amp Attack, Decay, Sustain, Release, Cutoff, Resonance, LFO Frequency, Poly Mod Filter Env | direct |
| Pads 1–5 | Osc A Saw, Osc A Pulse, Osc B Saw, Osc B Triangle, Osc B Pulse | toggle |
| Pads 6–8 | Sync, Unison, Filter Kybd | toggle |
| Pads b1–b3 | Poly Mod → Osc A Freq, → Osc A PW, → Filter | toggle |
| Pad b4 | Poly Mod Osc B 0 / 0.25 / 0.5 | cycle |
| Pads b5–b7 | Wheel Mod → Osc A Freq, → Osc B Freq, → Filter | toggle |
| Pad b8 | Glide 0 / 0.2 / 0.45 | cycle |
| Save, Quant, Undo | LFO Saw, Triangle, Square | toggle |
| Redo / Metronome | Osc B Kybd / Release | toggle |
| Mod wheel | Native: wheel mod amount | — |

## RF-7

| Control | Parameter | How |
|---|---|---|
| Knob 1 / 2 / 7 | Master Tune, Bend Range, LFO Delay | direct |
| Faders 1–5 | Envelope Time, Velocity Depth, Mod Wheel Range, Portamento, Brightness | direct |
| Fader 7 / 8 | LFO Rate, LFO Depth | direct |
| Pads 1–6 | Operators 1–6 on/off | toggle |
| Pad 7 / 8 | Poly ↔ Mono / Wheel target Pitch, Amp, Both, EG Bias | toggle / cycle |
| Pads b1–b3 | Transpose −12, 0, +12 | set |
| Pad b4 | Bend Range 2 / 7 / 12 | cycle |
| Pads b5–b7 | Envelope Time, Brightness, Velocity Depth | cycle |
| Pad b8 | Vibrato (LFO Depth 0.3) ↔ none | toggle |
| ⏪ ⏩ | Transpose a semitone down and up | step |
| Mod wheel | Native, per Mod Wheel Range and Target | — |

## RF-Tines

| Control | Parameter | How |
|---|---|---|
| Knobs 1–7 | Hammer Hardness, Bell, Sustain, Dynamics, Pickup Distance, Tine Alignment, Bass Boost | direct |
| Fader 1 / 2 | Bass, Treble | direct |
| Fader 7 / 8 | Vibrato Speed, Intensity | direct |
| Pad 1 / 2 | Vibrato on ↔ off / on while held | toggle / hold |
| Pad 3 / 4 | Panel Stage ↔ Suitcase / Pickup model | toggle / cycle |
| Pads 5–8 | Speed 2 / 4 / 7 Hz, Hardness, Bell, Intensity | cycle |
| Mod wheel | Vibrato Intensity | direct |

RF-Tines' Vibrato is the Suitcase's stereo tremolo. It sounds only when
Vibrato is on and the panel is Suitcase. Intensity sets its depth.
