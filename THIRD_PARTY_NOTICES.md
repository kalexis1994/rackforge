# Third-Party Notices

RackForge Standard builds include the officially pinned plugins as separately
packaged portable instruments and effects. Each `.rfplugin` archive retains its
own license and notice files, and each plugin remains a software component
separate from RackForge. The pinned versions are listed in
`tools/fetch-official-plugins.py`, which is the one place they are recorded.

## Officially pinned plugins

| Package | Project | License |
| --- | --- | --- |
| `RF-106.rfplugin` | <https://github.com/kalexis1994/rackforge-plugin-rf-106> | GPL-3.0-only |
| `RF-5.rfplugin` | <https://github.com/kalexis1994/rackforge-plugin-rf-5> | GPL-3.0-only |
| `RF-7.rfplugin` | <https://github.com/kalexis1994/RF-7> | GPL-3.0-only |
| `RF-Organ.rfplugin` | <https://github.com/kalexis1994/RF-Organ> | GPL-2.0-or-later |
| `RF-Tines.rfplugin` | <https://github.com/kalexis1994/RF-Tines> | Proprietary, all rights reserved |
| `RF-Limiter.rfplugin` | <https://github.com/kalexis1994/rackforge-plugin-rf-limiter> | GPL-3.0-only |
| `RF-EQ.rfplugin` | <https://github.com/kalexis1994/rackforge-plugin-rf-eq> | GPL-3.0-only |
| `RF-Comp.rfplugin` | <https://github.com/kalexis1994/rackforge-plugin-rf-comp> | GPL-3.0-only |

The complete license and notices of each plugin are distributed inside its own
package. RF-Tines is redistributed by permission of the copyright holder and is
not covered by RackForge's own license.

RackForge includes the Chakra Petch typeface for display typography.

## Chakra Petch

- Project: <https://github.com/m4rc1e/Chakra-Petch>
- License: SIL Open Font License 1.1

Copyright 2018 The Chakra Petch Project Authors.

The complete license is distributed with the Web and Android assets at
`fonts/OFL.txt`.

RackForge VST3 uses the `vst3` Rust bindings generated from the VST 3 API.

## vst3-rs

- Project: <https://github.com/coupler-rs/vst3-rs>
- Version: 0.3.0
- License: MIT OR Apache-2.0

Copyright (c) the vst3-rs contributors.

RackForge for Windows reaches ASIO drivers through a modified copy of
`asio-sys`, kept at `vendor/asio-sys`.

## asio-sys (modified)

- Project: <https://github.com/RustAudio/cpal/tree/master/asio-sys>
- Version: 0.2.6, modified by the RackForge project as `0.2.6+rackforge.1`
- License: Apache-2.0

Copyright (c) Tom Gowan and the cpal contributors. The modification counts
the ASIO driver's own reports of lost audio. `vendor/asio-sys/FORK.md` says
what changed, and the full license is at `vendor/asio-sys/LICENSE`.

RackForge's Web resource explorer includes the SVAR React File Manager and its
SVAR UI support packages.

## SVAR React File Manager

- Package: `@svar-ui/react-filemanager` 2.6.0
- Project: <https://github.com/svar-widgets/react-filemanager>
- License: MIT

Copyright (c) 2025 XB Software Sp. z o.o.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
