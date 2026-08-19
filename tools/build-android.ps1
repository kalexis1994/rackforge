[CmdletBinding()]
param(
    [string]$OutputDirectory = "dist/android"
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

# The Concert Grand ships inside the APK, exactly like the desktop bundle.
$bundledPlugin = Join-Path $repository "dist/bundled-plugins/RackForge-Concert-Grand.rfplugin"
$assetDirectory = Join-Path $androidProject "app/src/main/assets/bundled-plugins"
if (Test-Path -LiteralPath $bundledPlugin) {
    New-Item -ItemType Directory -Force $assetDirectory | Out-Null
    Copy-Item $bundledPlugin (Join-Path $assetDirectory "RackForge-Concert-Grand.rfplugin") -Force
} else {
    Write-Warning "dist/bundled-plugins/RackForge-Concert-Grand.rfplugin not found; the APK ships without the bundled piano."
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
New-Item -ItemType Directory -Force -Path $nativeOutput | Out-Null
Copy-Item -LiteralPath (Join-Path $repository "target/aarch64-linux-android/release/librackforge_android_native.so") `
    -Destination (Join-Path $nativeOutput "librackforge_android.so") -Force

$webOutput = Join-Path $androidProject "app/build/generated/web-ui/rackforge"
if (Test-Path -LiteralPath $webOutput) {
    Remove-Item -LiteralPath $webOutput -Recurse -Force
}
New-Item -ItemType Directory -Path $webOutput -Force | Out-Null
Copy-Item -Path (Join-Path $repository "web/dist/*") -Destination $webOutput -Recurse -Force

$bundledOutput = Join-Path $androidProject "app/build/generated/bundled-plugins"
if (Test-Path -LiteralPath $bundledOutput) {
    Remove-Item -LiteralPath $bundledOutput -Recurse -Force
}
if ($env:RACKFORGE_BUNDLED_PLUGIN) {
    if (-not (Test-Path -LiteralPath $env:RACKFORGE_BUNDLED_PLUGIN -PathType Leaf)) {
        throw "RACKFORGE_BUNDLED_PLUGIN is not a file: $env:RACKFORGE_BUNDLED_PLUGIN"
    }
    $bundledDirectory = Join-Path $bundledOutput "bundled"
    New-Item -ItemType Directory -Path $bundledDirectory -Force | Out-Null
    Copy-Item -LiteralPath $env:RACKFORGE_BUNDLED_PLUGIN `
        -Destination (Join-Path $bundledDirectory "RF-Soundfonts.rfplugin") -Force
}

Push-Location $androidProject
try {
    & $gradle assembleDebug --no-daemon
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
$output = Join-Path $repository $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null
$destination = Join-Path $output "RackForge-debug.apk"
Copy-Item -LiteralPath $source -Destination $destination -Force
Write-Host "RackForge Android: $destination"
