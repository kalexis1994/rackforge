# Control layouts of the RackForge instruments

A control layout says which of a plugin's parameters a keyboard's controls
move. It names **slots**, not controls: a plugin is laid out once for every
keyboard. Each controller package names the slot each of its controls fills,
and RackForge puts the two together into the map each keyboard is offered
(`crates/rackforge-core/src/controller_layouts.rs`).

The files here are the layouts of RackForge's six instruments. `rackforge-core`
embeds them for plugin packages that do not carry their own yet.

## A plugin's own layout

A plugin package lays itself out in `metadata/control-layout.json`. It is the
same document as the files here, and it replaces RackForge's layout for that
plugin. No manifest field points to the file, so a host that does not read it
still installs the package.

```json
{
  "format": "org.rackforge.control-layout",
  "schema_version": 1,
  "plugin_id": "org.example.synth",
  "plugin_name": "Example Synth",
  "slots": [
    { "slot": "control-1.1", "parameter_id": "cutoff", "mode": { "kind": "direct" } },
    { "slot": "switch-1.1", "parameter_id": "chorus",
      "mode": { "kind": "toggle", "first": 1, "second": 0 } },
    { "slot": "step.up", "parameter_id": "octave", "mode": { "kind": "step", "direction": "up" } }
  ]
}
```

Rules:

- A slot is taken once.
- A continuous slot takes a position mode (direct, range, zones). A switch or a
  step takes a button mode.
- Parameters are named by their stable schema `id`.
- A layout names no parameter of another plugin.

## Slots

| Slot | What goes there | Filled by, for example |
|---|---|---|
| `control-1.1`–`control-1.8` | The eight continuous controls played most | faders; the knobs of a keyboard without faders |
| `control-2.1`–`control-2.8` | The next eight | the knobs above the faders |
| `control-3.1`–`control-3.8` | Eight more | a second encoder page, a Launch Control's second row |
| `switch-1.1`–`switch-1.8` | The switches played most | pads bank 1 (notes 40–43, 36–39), a Launch Control's top buttons |
| `switch-2.1`–`switch-2.8` | Second-level switches and sound variants | pads bank 2 (notes 48–51, 44–47) |
| `switch-3.1`–`switch-3.8` | A keyboard's labelled buttons | Save/Capture, Quantise, Metronome/Click, Undo |
| `step.down`, `step.up` | Step the main selector | ⏪ ⏩, track ◀ ▶ |
| `step-2.down`, `step-2.up` | Step a second selector | Undo/Redo on the KeyLab, pad ▼ ▲ on a Launchkey |
| `mod-wheel` | The modulation wheel | |

A keyboard may lack a whole row of continuous controls. The parameters of that
row then move, in order, onto the controls of the rows it does have that the
layout leaves empty. For example, a keyboard with eight knobs and no faders
plays the organ's drawbars on its knobs. RF-Tines, whose first row has gaps,
fills them with its hammer and bell. Switches and steps stay in their slot.

## The same rules across the six instruments

- **Row 1 sets levels and shape:** drawbars, envelopes, cutoff, room.
- **Row 2 sets tone and colour.**
- **Row 3 holds what the KeyLab has no room for.**
- **Switch bank 1 holds the switches played most.**
- **Switch bank 2 holds the second-level switches and sound variants.** A
  cycle steps through a few chosen values, and a toggle goes to its first
  value and back.
- **Step down and up move a selector** (preset key, range, transpose).
- **The mod wheel is left to the instruments that handle CC 1 themselves:**
  RF-Organ (Leslie speed), RF-106 (the bender lever's LFO), RF-5 (wheel mod)
  and RF-7 (wheel range and target). Only RF - Concert Grand and RF-Tines,
  which ignore CC 1, lay it out.
- **The master level and pan are not laid out.** The KeyLab's ninth knob and
  fader, and every keyboard's master fader, keep them.

A mapped pad does not also play its note.

## RF-Organ

| Slots | Parameter | How |
|---|---|---|
| control-1.1–1.8 | Upper drawbars B 16' … 1 1/3' | direct |
| control-2.1–2.7 | Expression, Preamp Drive, Key Click, Rotary Mix, Rotor Inertia, Bass Trim, AO-28 Tone | direct |
| control-2.8 | Upper drawbar B 1' | direct |
| control-3.1–3.8 | Generator Leakage, Contact Bounce, Transformer Drive, Horn Level, Drum Level, Horn Mic Distance, Cabinet Reflections, Mic Pattern | direct |
| switch-1.1 / 1.2 | Rotary: Chorale ↔ Tremolo / Brake | toggle / set |
| switch-1.3–1.6 | Percussion on, Harmonic 2nd/3rd, Volume Soft/Normal, Decay Slow/Fast | toggle |
| switch-1.7 / 1.8 | Upper Vibrato on / Scanner V1…C3 | toggle / cycle |
| switch-2.1–2.4 | Preset key B, A#, F (Flutes 8' & 4'), A (Full Swell) | set |
| switch-2.5 / 2.6 / 2.7 | Leslie off ↔ Chorale / Lower Vibrato / Mic Dynamic ↔ Condenser | toggle |
| switch-2.8 | Tremolo while held, Chorale on release | hold |
| step / step-2 | Preset key / Scanner mode, down and up | step |

## RF - Concert Grand

| Slots | Parameter | How |
|---|---|---|
| control-1.1–1.8 | Brightness, Dynamics, Decay, Unison, Ambience Level, Room Size, Mic Distance, Stereo Width | direct |
| control-2.1–2.8 | Hammer Hard, Hammer Mass, Detune, Sympathy, Treble Life, Lid, Wall Hardness, Mic Pattern | direct |
| control-3.1–3.8 | Felt Corner, Bloom, Clang, Phantoms, Prompt Decay, Tail, Board, Thud Colour | direct |
| switch-1.1 | Action Grand ↔ Upright | toggle |
| switch-1.2–1.4 | Room 100 / 311 / 3000 / 15000 m³, Mic 0.66 / 2 / 6 m, Lid open / half / closed | cycle |
| switch-1.5–1.7 | Action, Release, Pedal noise off ↔ default | toggle |
| switch-1.8 | Stereo Width mono ↔ default | toggle |
| switch-2.1, 2.2 | Brightness 0.3…0.75, Dynamics 0.25…0.7 | cycle |
| switch-2.3 / 2.4 | Honky-tonk detune 0.9 ↔ 0.5 / Dry (no ambience) ↔ 0.5 | toggle |
| switch-2.5–2.8 | Wall Hardness, Mic Pattern, Decay, Sympathy | cycle |
| mod-wheel | Ambience Level | direct |

## RF-106

| Slots | Parameter | How |
|---|---|---|
| control-1.1–1.8 | Attack, Decay, Sustain, Release, Cutoff, Resonance, LFO Rate, DCO LFO | direct |
| control-2.1–2.8 | PWM, Sub, Noise, VCF Env, VCF LFO, VCF Kybd, LFO Delay, VCA Level | direct |
| control-3.1–3.5 | Portamento, Bender DCO, Bender VCF, Bender LFO, Tune | direct |
| switch-1.1, 1.2 | Pulse, Saw | toggle |
| switch-1.3 / 1.4 | Chorus I / Chorus II (press again for off) | toggle |
| switch-1.5–1.7 | PWM LFO ↔ Manual, VCF Env + ↔ −, VCA Env ↔ Gate | toggle |
| switch-1.8 | HPF 0…3 | cycle |
| switch-2.1 | LFO trigger, the lever push, while held | trigger |
| switch-2.2 / 2.3 | Portamento on / Assign Unison, Poly 1, Poly 2 | toggle / cycle |
| step | Range 16' / 8' / 4' | step |

## RF-5

| Slots | Parameter | How |
|---|---|---|
| control-1.1–1.8 | Amp Attack, Decay, Sustain, Release, Cutoff, Resonance, LFO Frequency, Poly Mod Filter Env | direct |
| control-2.1–2.4 | Osc A PW, Osc B Fine, Osc B Frequency, Filter Env Amount | direct |
| control-2.5–2.8 | Filter Attack, Decay, Sustain, Release | direct |
| control-3.1–3.8 | Osc A Level, Osc B Level, Noise, Osc A Frequency, Osc B PW, Poly Mod Osc B, Wheel Mod Source Mix, Glide | direct |
| switch-1.1–1.5 | Osc A Saw, Osc A Pulse, Osc B Saw, Osc B Triangle, Osc B Pulse | toggle |
| switch-1.6–1.8 | Sync, Unison, Filter Kybd | toggle |
| switch-2.1–2.3 | Poly Mod → Osc A Freq, → Osc A PW, → Filter | toggle |
| switch-2.4 | Poly Mod Osc B 0 / 0.25 / 0.5 | cycle |
| switch-2.5–2.7 | Wheel Mod → Osc A Freq, → Osc B Freq, → Filter | toggle |
| switch-2.8 | Glide 0 / 0.2 / 0.45 | cycle |
| switch-3.1 / 3.2 / 3.3 | LFO Saw, LFO Triangle, Release | toggle |
| step-2.down / up | LFO Square / Osc B Kybd | toggle |

## RF-7

| Slots | Parameter | How |
|---|---|---|
| control-1.1–1.5 | Envelope Time, Velocity Depth, Mod Wheel Range, Portamento, Brightness | direct |
| control-1.7 / 1.8 | LFO Rate, LFO Depth | direct |
| control-2.1 / 2.2 / 2.7 | Master Tune, Bend Range, LFO Delay | direct |
| control-3.1 | Aftertouch Range | direct |
| switch-1.1–1.6 | Operators 1–6 on/off | toggle |
| switch-1.7 / 1.8 | Poly ↔ Mono / Wheel target Pitch, Amp, Both, EG Bias | toggle / cycle |
| switch-2.1–2.3 | Transpose −12, 0, +12 | set |
| switch-2.4 | Bend Range 2 / 7 / 12 | cycle |
| switch-2.5–2.7 | Envelope Time, Brightness, Velocity Depth | cycle |
| switch-2.8 | Vibrato (LFO Depth 0.3) ↔ none | toggle |
| step | Transpose a semitone down and up | step |

## RF-Tines

| Slots | Parameter | How |
|---|---|---|
| control-1.1 / 1.2 | Bass, Treble | direct |
| control-1.7 / 1.8 | Vibrato Speed, Intensity | direct |
| control-2.1–2.7 | Hammer Hardness, Bell, Sustain, Dynamics, Pickup Distance, Tine Alignment, Bass Boost | direct |
| switch-1.1 / 1.2 | Vibrato on ↔ off / on while held | toggle / hold |
| switch-1.3 / 1.4 | Panel Stage ↔ Suitcase / Pickup model | toggle / cycle |
| switch-1.5–1.8 | Speed 2 / 4 / 7 Hz, Hardness, Bell, Intensity | cycle |
| mod-wheel | Vibrato Intensity | direct |

RF-Tines' Vibrato is the Suitcase's stereo tremolo. It sounds only when
Vibrato is on and the panel is Suitcase. Intensity sets its depth.
