# Platform parity

RackForge runs the same instruments and performances on every host it supports. This
table is where that promise is kept honest: it is generated from
`crates/rackforge-host-capabilities`, and a test fails if the two disagree, so it
cannot go stale. Do not edit it by hand — change the declaration and run
`cargo test -p rackforge-host-capabilities`.

`yes` means implemented, `planned` means a known gap with a reason below, `no` means it
cannot exist on that host, and `unaudited` means nobody has checked yet.

| Capability | Windows | Android | Raspberry Pi | Browser |
| --- | --- | --- | --- | --- |
| Choose an instrument and play it | yes | yes | yes | yes |
| Choose the program an instrument plays | yes | yes | yes | yes |
| Set master level and pan | yes | yes | yes | yes |
| Play from an on-screen keyboard or pads | yes | yes | yes | yes |
| Play from a connected MIDI controller | yes | yes | yes | yes |
| Send MIDI to hardware | unaudited | unaudited | yes | planned |
| Notice controllers connecting while running | unaudited | unaudited | yes | planned |
| Read and write plugin parameters | yes | yes | yes | yes |
| Save, load, rename and delete host presets | yes | yes | yes | yes |
| Create, preview and save Custom Programs | yes | unaudited | yes | planned |
| Audition a program and keep the selected one | yes | unaudited | yes | planned |
| Create and edit Racks, Songs and Setlists | yes | yes | yes | yes |
| Play a Rack, with every slot rendered | unaudited | unaudited | yes | planned |
| Edit one Rack slot without disturbing PLAY | yes | unaudited | yes | planned |
| Install a portable .rfplugin package | yes | yes | yes | yes |
| Remove an installed plugin and its data | yes | yes | yes | yes |
| Give a plugin a sound library or ROM it declares | yes | yes | yes | planned |
| Show a plugin's own PLAY and CONFIG interfaces | yes | yes | yes | yes |
| Drive hardware surfaces from a .rfcontroller | unaudited | unaudited | yes | planned |
| Choose the audio device and buffer size | yes | yes | yes | no |
| Keep instruments and performances between runs | yes | yes | yes | yes |
| Restore the previous session on the next start | yes | yes | yes | planned |
| Work with no network connection | yes | yes | yes | yes |
| Survive a plugin that stops responding | yes | yes | yes | no |

## Why a capability is missing

### Browser

- **Send MIDI to hardware** (planned): Web MIDI outputs are not connected yet; controller surfaces need them
- **Notice controllers connecting while running** (planned): Web MIDI reports devices arriving, but the host does not listen yet
- **Create, preview and save Custom Programs** (planned): the program-draft commands are not implemented in the browser host
- **Audition a program and keep the selected one** (planned): audition leases are not implemented in the browser host
- **Play a Rack, with every slot rendered** (planned): the page renders the active PLAY instrument; Rack slots are not mixed yet
- **Edit one Rack slot without disturbing PLAY** (planned): isolated plugin state is not exposed by the browser host yet
- **Give a plugin a sound library or ROM it declares** (planned): the host installs a chosen file into a plugin's private storage and reloads it, but no packaged plugin here asks for one, so the path a plugin's own interface takes is unproven
- **Drive hardware surfaces from a .rfcontroller** (planned): blocked on MIDI output, and unavailable in browsers without Web MIDI
- **Choose the audio device and buffer size** (no): a page renders into the output the browser gives it and cannot enumerate or configure audio hardware
- **Restore the previous session on the next start** (planned): the session checkpoint is not written by the browser host yet
- **Survive a plugin that stops responding** (no): the engine inside a page does not meter guest execution, so a plugin that stops responding blocks the audio callback

