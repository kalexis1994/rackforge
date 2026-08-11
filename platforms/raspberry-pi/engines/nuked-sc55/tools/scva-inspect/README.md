# scva-inspect

Read-only Rust tool for static analysis of files from a legitimately installed
copy of Sound Canvas VA.

It can:

- identify PE architecture and sections;
- compute SHA-256 hashes and entropy;
- enumerate imports and exports;
- locate relevant strings and data candidates;
- recognize the observed SCCore 1.1.2 wave layout;
- compare candidate sizes/hashes with known SC-55mkII material;
- explicitly extract bounded candidates with reproducible manifests.

The normal mode never loads or executes a DLL. Extraction requires an explicit
empty output directory and writes each result atomically through a temporary
file.

Extracted output is proprietary material derived from the user's installation.
It must remain below ignored local directories and must never be committed,
uploaded, or bundled with RackForge.
