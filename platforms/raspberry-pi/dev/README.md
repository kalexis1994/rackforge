# Raspberry Pi development tools

The local repository is the source of truth. The Pi directory
`$HOME/rackforge/current` is a deployable copy and may be rebuilt at any time.

Private keys remain outside the repository. Copy `ssh_config.example` into the
user's SSH configuration after setting `HostName` and `User`; project tools
require non-interactive key authentication.

Tools:

- `bootstrap.sh`: initial target-side dependencies and directories.
- `connect.ps1`: open a remote shell.
- `sync.ps1`: package and deploy tracked project content while excluding local
  artifacts.
- `health.ps1`: inspect services, devices, audio, and recent logs.
- `install-nuked-roms.ps1`: copy user-authorized Nuked-SC55 ROMs.
- `install-scva-banks.ps1`: copy user-authorized SCVA-derived bank data.
- `render-scva-bank.ps1`: drive the local Windows research renderer.

No tool copies SSH keys, passwords, proprietary banks, or runtime user state
into Git.
