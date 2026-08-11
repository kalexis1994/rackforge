# scva-render

Isolated Windows x64 probe for the C ABI exported by `SCCore.dll` from a
legitimate Sound Canvas VA installation. It sends configurable Program Change
and note events, renders four seconds of 44.1 kHz stereo audio, and writes a
32-bit float WAV oracle.

The tool is research-only. It does not translate the DLL to ARM and does not
redistribute proprietary code, banks, or rendered output.

`--patch-u32 OFFSET VALUE` creates a temporary DLL copy, changes four bytes
before loading it, and deletes the copy afterward. It never modifies the
original installation. Use it only for bounded experiments against known table
fields.

The DPCM inspection mode compares selected internal accumulators without
rendering audio. All resulting files remain local and ignored.
