# Sound engines

Sound engines sit behind the RackForge runtime. The host owns device selection,
lifecycle, programs, session state, and controller feedback; an engine receives
MIDI/control data and produces audio.

Current tracks:

1. RF-DLS: active General MIDI instrument distributed from its own repository
   and using a user-provided DLS bank.
2. Nuked-SC55: research into accurate Roland SC-55 emulation.
3. `scva-arm64`: native reader/renderer research for user-authorized SCVA
   bank data.

Evaluation criteria include Linux ARM64 source availability, headless/block API,
real-time behavior, licensing, external-bank legality, and deterministic
offline tests. Proprietary ROMs, banks, and extracted audio are never committed
or distributed by RackForge.
