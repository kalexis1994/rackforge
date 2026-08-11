# Nuked-SC55 integration

RackForge vendors Nuked-SC55 as a pinned submodule for research into Roland
Sound Canvas emulation. It emulates the original MCU and PCM device; it is not a
SoundFont and does not run the Windows plugin through a compatibility layer.

The upstream MAME-style license permits non-commercial use and redistribution
under its terms but is not suitable as the foundation of a commercial product.
Review the pinned upstream license before redistribution.

Nuked-SC55 does not include Roland firmware or wave ROMs. RackForge neither
downloads nor versions them. Users must provide material they are legally
allowed to use and keep it outside Git.

Expected local layout:

```text
local/roms/nuked-sc55/
├── mk1/
└── mk2/
```

Use the provided ROM check script to validate presence and sizes without
modifying files. Build artifacts and user ROMs must remain ignored.

This integration is an optional research/runtime backend and does not define the
portable `.rfplugin` contract.
