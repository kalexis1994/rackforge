#!/usr/bin/env bash
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
android_project="$repository/apps/rackforge-android"
sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
ndk_version="27.0.12077973"
output_directory="${1:-$repository/dist/android}"

[[ -n "$sdk_root" ]] || {
  printf 'ANDROID_SDK_ROOT or ANDROID_HOME must point to the Android SDK.\n' >&2
  exit 2
}
ndk_root="$sdk_root/ndk/$ndk_version"
toolchain="$ndk_root/toolchains/llvm/prebuilt/linux-x86_64/bin"
clang="$toolchain/aarch64-linux-android26-clang"
clangxx="$toolchain/aarch64-linux-android26-clang++"
archive_tool="$toolchain/llvm-ar"

[[ -x "$clang" && -x "$clangxx" && -x "$archive_tool" ]] || {
  printf 'Android NDK %s is not installed below %s.\n' "$ndk_version" "$sdk_root" >&2
  exit 2
}
[[ -f "$android_project/gradlew" ]] || {
  printf 'Gradle wrapper is missing at %s.\n' "$android_project/gradlew" >&2
  exit 2
}
command -v pnpm >/dev/null 2>&1 || {
  printf 'pnpm is required to build the shared RackForge UI.\n' >&2
  exit 2
}

export ANDROID_HOME="$sdk_root"
export ANDROID_SDK_ROOT="$sdk_root"
export ANDROID_NDK_ROOT="$ndk_root"
export PATH="$toolchain:$PATH"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$archive_tool"
export CC_aarch64_linux_android="$clang"
export CXX_aarch64_linux_android="$clangxx"
export AR_aarch64_linux_android="$archive_tool"

cd "$repository"
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
rustup target add aarch64-linux-android
cargo build --locked --release \
  -p rackforge-android-native \
  --target aarch64-linux-android

native_output="$android_project/app/build/generated/rust-jni/arm64-v8a"
install -d "$native_output"
install -m 0644 \
  "$repository/target/aarch64-linux-android/release/librackforge_android_native.so" \
  "$native_output/librackforge_android.so"

bundled_output="$android_project/app/build/generated/bundled-plugins"
rm -rf -- "$bundled_output"
if [[ -n "${RACKFORGE_BUNDLED_PLUGIN:-}" ]]; then
  [[ -f "$RACKFORGE_BUNDLED_PLUGIN" ]] || {
    printf 'RACKFORGE_BUNDLED_PLUGIN is not a file: %s\n' \
      "$RACKFORGE_BUNDLED_PLUGIN" >&2
    exit 2
  }
  install -d "$bundled_output/bundled"
  install -m 0644 "$RACKFORGE_BUNDLED_PLUGIN" \
    "$bundled_output/bundled/RackForge-Concert-Grand.rfplugin"
fi

web_output="$android_project/app/build/generated/web-ui/rackforge"
rm -rf -- "$web_output"
install -d "$web_output"
cp -R "$repository/web/dist/." "$web_output/"

cd "$android_project"
bash ./gradlew assembleDebug --no-daemon

source_apk="$android_project/app/build/outputs/apk/debug/app-debug.apk"
[[ -f "$source_apk" ]] || {
  printf 'Gradle completed without producing %s.\n' "$source_apk" >&2
  exit 1
}
mkdir -p "$output_directory"
output_directory="$(cd "$output_directory" && pwd)"
install -m 0644 "$source_apk" "$output_directory/RackForge-debug.apk"
printf 'RackForge Android: %s\n' "$output_directory/RackForge-debug.apk"
