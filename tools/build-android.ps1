[CmdletBinding()]
param(
    [string]$OutputDirectory = "dist/android",
    [ValidateSet("Standard", "Minimal")]
    [string]$Edition = "Standard"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$toolRoot = Join-Path $repository "local/android-toolchain"
$sdkRoot = if ($env:ANDROID_SDK_ROOT) {
    $env:ANDROID_SDK_ROOT
} elseif ($env:ANDROID_HOME) {
    $env:ANDROID_HOME
} else {
    Join-Path $toolRoot "sdk"
}
$jdkRoot = if ($env:JAVA_HOME) {
    $env:JAVA_HOME
} else {
    Get-ChildItem (Join-Path $toolRoot "jdk") -Directory -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
$androidProject = Join-Path $repository "apps/rackforge-android"
$gradle = Join-Path $androidProject "gradlew.bat"
$ndkRoot = Join-Path $sdkRoot "ndk/27.0.12077973"

if (-not $jdkRoot -or -not (Test-Path -LiteralPath (Join-Path $jdkRoot "bin/java.exe"))) {
    throw "Local JDK not found below $toolRoot."
}
if (-not (Test-Path -LiteralPath (Join-Path $sdkRoot "platforms/android-36/android.jar"))) {
    throw "Android SDK platform 36 not found below $sdkRoot."
}
if (-not (Test-Path -LiteralPath $gradle)) {
    throw "Gradle wrapper not found at $gradle."
}
if (-not (Test-Path -LiteralPath $ndkRoot)) {
    throw "Android NDK 27.0.12077973 not found below $sdkRoot."
}

$officialPlugins = ""
if ($Edition -eq "Standard") {
    $officialPlugins = Join-Path $repository "dist/bundled-plugins/official"
    & python (Join-Path $repository "tools/fetch-official-plugins.py") `
        --output-directory $officialPlugins
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge official plugin download failed."
    }
}

$env:JAVA_HOME = $jdkRoot
$env:ANDROID_HOME = $sdkRoot
$env:ANDROID_SDK_ROOT = $sdkRoot
$ndkBin = Join-Path $ndkRoot "toolchains/llvm/prebuilt/windows-x86_64/bin"
$androidClang = Join-Path $ndkBin "aarch64-linux-android26-clang.cmd"
$androidClangCpp = Join-Path $ndkBin "aarch64-linux-android26-clang++.cmd"
$androidAr = Join-Path $ndkBin "llvm-ar.exe"
$env:PATH = "$ndkBin;$env:PATH"
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $androidClang
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_AR = $androidAr
$env:CC_aarch64_linux_android = $androidClang
$env:CXX_aarch64_linux_android = $androidClangCpp
$env:AR_aarch64_linux_android = $androidAr

# Each edition must start from a clean Android package graph. Gradle's
# incremental APK writer can otherwise leave removed plugin bytes behind after
# switching from Standard to Minimal, even when those files no longer appear
# in the ZIP directory.
Push-Location $androidProject
try {
    & $gradle clean --no-daemon
    if ($LASTEXITCODE -ne 0) {
        throw "Android clean failed."
    }
} finally {
    Pop-Location
}

Push-Location $repository
try {
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
        throw "pnpm is required to build the shared RackForge UI."
    }
    & pnpm --dir web install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge shared UI dependency installation failed."
    }
    & pnpm --dir web build
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge shared UI build failed."
    }
    # Use the MSVC host toolchain explicitly. The user's default Rust host may be
    # GNU, which makes native build dependencies look for dlltool.exe even though
    # the final target is linked by the Android NDK clang toolchain configured above.
    & rustup run stable-x86_64-pc-windows-msvc cargo build --locked --release `
        -p rackforge-android-native --target aarch64-linux-android
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge Android native runtime build failed."
    }
} finally {
    Pop-Location
}

$nativeOutput = Join-Path $androidProject "app/build/generated/rust-jni/arm64-v8a"
if (Test-Path -LiteralPath $nativeOutput) {
    Remove-Item -LiteralPath $nativeOutput -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $nativeOutput | Out-Null
$nativeLibrary = Join-Path $nativeOutput "librackforge_android.so"
Copy-Item -LiteralPath (Join-Path $repository "target/aarch64-linux-android/release/librackforge_android_native.so") `
    -Destination $nativeLibrary -Force

# Binaryen (wasm-opt, in the plugin runtime) is C++ and links the NDK's
# shared C++ runtime, which Android does not provide: it travels in the APK
# beside the library. Without it the app died on launch, unable to load
# librackforge_android.so. Every library the runtime needs is then checked:
# either Android provides it (the NDK carries a stub for it at the minimum
# API level) or it is packaged here.
$sysrootLib = Join-Path $ndkRoot "toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android"
Copy-Item -LiteralPath (Join-Path $sysrootLib "libc++_shared.so") `
    -Destination (Join-Path $nativeOutput "libc++_shared.so") -Force
$neededLibraries = & (Join-Path $ndkBin "llvm-readelf.exe") --needed-libs $nativeLibrary |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -match '^lib\S*\.so$' }
if ($LASTEXITCODE -ne 0 -or -not $neededLibraries) {
    throw "Could not list the libraries librackforge_android.so needs."
}
foreach ($needed in $neededLibraries) {
    $systemStub = Join-Path $sysrootLib "26/$needed"
    $packaged = Join-Path $nativeOutput $needed
    if (-not (Test-Path -LiteralPath $systemStub) -and -not (Test-Path -LiteralPath $packaged)) {
        throw "librackforge_android.so needs $needed, which neither Android nor the APK provides."
    }
}

$webOutput = Join-Path $androidProject "app/build/generated/web-ui/rackforge"
if (Test-Path -LiteralPath $webOutput) {
    Remove-Item -LiteralPath $webOutput -Recurse -Force
}
New-Item -ItemType Directory -Path $webOutput -Force | Out-Null
Copy-Item -Path (Join-Path $repository "web/dist/*") -Destination $webOutput -Recurse -Force

# Older RackForge builds copied the bundled piano into the source asset tree.
# It is ignored by Git, but Gradle still merges it and reports a duplicate now
# that all bundled packages are produced under app/build/generated. Remove only
# that known legacy build artifact; user-installed plugins never live here.
$legacyBundledPlugin = Join-Path $androidProject `
    "app/src/main/assets/bundled-plugins/RackForge-Concert-Grand.rfplugin"
if (Test-Path -LiteralPath $legacyBundledPlugin) {
    Remove-Item -LiteralPath $legacyBundledPlugin -Force
}

$bundledOutput = Join-Path $androidProject "app/build/generated/bundled-plugins"
if (Test-Path -LiteralPath $bundledOutput) {
    Remove-Item -LiteralPath $bundledOutput -Recurse -Force
}
$bundledDirectory = Join-Path $bundledOutput "bundled-plugins"
New-Item -ItemType Directory -Path $bundledDirectory -Force | Out-Null
$defaultPlugin = ""
if ($Edition -eq "Standard") {
    $defaultPlugin = $env:RACKFORGE_BUNDLED_PLUGIN
    if (-not $defaultPlugin) {
        # The Standard edition opens on the Concert Grand. It is built from
        # this repository rather than fetched, so a local build makes it when
        # it is missing -- skipping it silently shipped an APK without the
        # piano, which then opened on whichever plugin sorted first.
        $defaultPlugin = Join-Path $repository "dist/bundled-plugins/RF-Concert-Grand.rfplugin"
        if (-not (Test-Path -LiteralPath $defaultPlugin -PathType Leaf)) {
            Push-Location $repository
            try {
                & rustup target add wasm32-unknown-unknown
                if ($LASTEXITCODE -ne 0) { throw "Could not install the WebAssembly Rust target." }
                & cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
                if ($LASTEXITCODE -ne 0) { throw "Concert Grand WebAssembly build failed." }
                New-Item -ItemType Directory -Force (Split-Path -Parent $defaultPlugin) | Out-Null
                & cargo run --release -p rackforge-store -- pack-wasm `
                    plugins/concert-grand/package `
                    target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm `
                    $defaultPlugin
                if ($LASTEXITCODE -ne 0) { throw "Concert Grand package build failed." }
            } finally {
                Pop-Location
            }
        }
    }
    if ($defaultPlugin) {
        if (-not (Test-Path -LiteralPath $defaultPlugin -PathType Leaf)) {
            throw "RACKFORGE_BUNDLED_PLUGIN is not a file: $defaultPlugin"
        }
        Copy-Item -LiteralPath $defaultPlugin `
            -Destination (Join-Path $bundledDirectory (Split-Path -Leaf $defaultPlugin)) -Force
    }
}
if ($Edition -eq "Standard") {
    Copy-Item -Path (Join-Path $officialPlugins "*.rfplugin") `
        -Destination $bundledDirectory -Force
}

Push-Location $androidProject
try {
    & $gradle testDebugUnitTest assembleDebug --no-daemon
    if ($LASTEXITCODE -ne 0) {
        throw "Android build failed."
    }
} finally {
    Pop-Location
}

$source = Join-Path $androidProject "app/build/outputs/apk/debug/app-debug.apk"
if (-not (Test-Path -LiteralPath $source)) {
    throw "Gradle completed without producing $source."
}
$output = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory
} else {
    Join-Path $repository $OutputDirectory
}
New-Item -ItemType Directory -Force -Path $output | Out-Null
$destination = Join-Path $output "RackForge-debug.apk"
Copy-Item -LiteralPath $source -Destination $destination -Force
"edition=$($Edition.ToLowerInvariant())" | Set-Content `
    -LiteralPath (Join-Path $output "build-info.txt") -Encoding utf8NoBOM
Write-Host "RackForge Android ($Edition): $destination"
