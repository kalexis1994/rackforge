# rackforge-repository

Portable repository client and package installer used by RackForge hosts. It
authenticates detached Ed25519 catalogs, verifies artifacts, safely extracts
`.rfplugin` ZIP archives and installs immutable versions side by side.

The crate never loads native code and must not run on the audio thread.
