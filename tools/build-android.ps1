[CmdletBinding()]
param(
    [string]$PluginPackage = "",
    [string]$OutputDirectory = "dist/android"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$toolRoot = Join-Path $repository "local/android-toolchain"
$sdkRoot = Join-Path $toolRoot "sdk"
$jdkRoot = Get-ChildItem (Join-Path $toolRoot "jdk") -Directory |
    Select-Object -First 1 -ExpandProperty FullName
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
    & cargo build --locked --release -p rackforge-android-native --target aarch64-linux-android
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

$arguments = @("assembleDebug", "--no-daemon")
if ($PluginPackage) {
    $resolvedPlugin = (Resolve-Path -LiteralPath $PluginPackage).Path
    $arguments += "-Prfplugin=$resolvedPlugin"
}

Push-Location $androidProject
try {
    & $gradle @arguments
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
