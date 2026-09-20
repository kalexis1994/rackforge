<#
.SYNOPSIS
  Cross-builds the appliance binary for the Raspberry Pi from Windows.

.DESCRIPTION
  `build-raspberry-pi.sh` builds on the Pi itself, which takes about
  seventeen minutes for a clean build and holds the appliance hostage while
  it runs. This builds the same binary here in roughly two.

  It also exists for a second, sharper reason. `crates/rackforge-core/src/
  live.rs` is `#[cfg(target_os = "linux")]`, so a host build on Windows does
  not compile it at all -- and says "Finished" anyway. A change to that file
  can be typo-free by cargo's account here and fail on the appliance, which
  is exactly how a commit once shipped with five missing struct fields. Any
  edit to live.rs (or anything else behind a Linux cfg) has to be checked
  with this script, because nothing else on this machine looks at it.

.NOTES
  Requires, once:
    cargo install cargo-zigbuild
    pip install ziglang
  and a sysroot with ALSA copied from the appliance:
    C:/sysroot-aarch64/usr/include
    C:/sysroot-aarch64/usr/lib/aarch64-linux-gnu
    C:/sysroot-aarch64/usr/lib/aarch64-linux-gnu/pkgconfig/alsa.pc
  plus the pkg-config shim at C:/Users/Administrator/bin/pkg-config.cmd.

  The GLIBC suffix on the triple is what lets a newer toolchain produce a
  binary the appliance's glibc will load; 2.41 is what the Pi runs.
#>
[CmdletBinding()]
param(
  [string] $Sysroot = "C:/sysroot-aarch64",
  [string] $PkgConfigShim = "C:/Users/Administrator/bin/pkg-config.cmd",
  [string] $Target = "aarch64-unknown-linux-gnu.2.41"
)

$ErrorActionPreference = "Stop"

foreach ($required in @($Sysroot, $PkgConfigShim,
                        "$Sysroot/usr/lib/aarch64-linux-gnu/pkgconfig/alsa.pc")) {
  if (-not (Test-Path $required)) {
    throw "falta $required -- ver las notas de este script"
  }
}

$env:PKG_CONFIG_SYSROOT_DIR = $Sysroot
$env:PKG_CONFIG_PATH = "$Sysroot/usr/lib/aarch64-linux-gnu/pkgconfig"
$env:PKG_CONFIG_ALLOW_CROSS = "1"
# The full path, extension included. Rust's process search appends `.exe` and
# nothing else, so a `.cmd` sitting on PATH is invisible to it: alsa-sys will
# report "The pkg-config command could not be found" while `Get-Command
# pkg-config` happily prints the shim. Naming it outright sidesteps the
# search. (A cached build script hides this until something changes an env
# var and cargo re-runs it, so the failure arrives long after the setup.)
$env:PKG_CONFIG = $PkgConfigShim

cargo zigbuild --release -p rackforge-core --target $Target
if ($LASTEXITCODE -ne 0) { throw "la compilacion cruzada fallo" }

$binary = "target/$($Target.Split('.')[0])/release/rackforge-core"
if (-not (Test-Path $binary)) { throw "no quedo binario en $binary" }
Write-Output $binary
