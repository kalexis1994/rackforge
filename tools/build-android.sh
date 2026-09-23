#!/usr/bin/env bash
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
android_project="$repository/apps/rackforge-android"
sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
ndk_version="27.0.12077973"
# Resolved before anything changes directory: a relative argument means the
# same place as the default, not wherever the build happens to be standing
# when it gets around to creating it. This script moves into the Android
# project, which is how a relative output landed under apps/rackforge-android.
output_directory="${1:-$repository/dist/android}"
case "$output_directory" in
  /*) ;;
  *) output_directory="$repository/$output_directory" ;;
esac
edition="${RACKFORGE_EDITION:-standard}"

case "$edition" in
  standard|minimal) ;;
  *)
    printf 'RACKFORGE_EDITION must be standard or minimal, got %s.\n' \
      "$edition" >&2
    exit 2
    ;;
esac

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
command -v python3 >/dev/null 2>&1 || {
  printf 'python3 is required to fetch official RackForge plugins.\n' >&2
  exit 2
}

official_plugins="$repository/dist/bundled-plugins/official"
if [[ "$edition" == standard ]]; then
  python3 "$repository/tools/fetch-official-plugins.py" \
    --output-directory "$official_plugins"
fi

export ANDROID_HOME="$sdk_root"
export ANDROID_SDK_ROOT="$sdk_root"
export ANDROID_NDK_ROOT="$ndk_root"
export PATH="$toolchain:$PATH"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$archive_tool"
export CC_aarch64_linux_android="$clang"
export CXX_aarch64_linux_android="$clangxx"
export AR_aarch64_linux_android="$archive_tool"

cd "$android_project"
bash ./gradlew clean --no-daemon

cd "$repository"
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
rustup target add aarch64-linux-android
cargo build --locked --release \
  -p rackforge-android-native \
  --target aarch64-linux-android

native_output="$android_project/app/build/generated/rust-jni/arm64-v8a"
rm -rf -- "$native_output"
install -d "$native_output"
install -m 0644 \
  "$repository/target/aarch64-linux-android/release/librackforge_android_native.so" \
  "$native_output/librackforge_android.so"

# Binaryen (wasm-opt, in the plugin runtime) is C++ and links the NDK's
# shared C++ runtime, which Android does not provide: it travels in the APK
# beside the library. Without it the app died on launch, unable to load
# librackforge_android.so. Every library the runtime needs is then checked:
# either Android provides it (the NDK carries a stub for it at the minimum
# API level) or it is packaged here.
sysroot_lib="$ndk_root/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android"
install -m 0644 "$sysroot_lib/libc++_shared.so" "$native_output/libc++_shared.so"
while read -r needed; do
  [[ -n "$needed" ]] || continue
  if [[ ! -f "$sysroot_lib/26/$needed" && ! -f "$native_output/$needed" ]]; then
    printf 'librackforge_android.so needs %s, which neither Android nor the APK provides.\n' "$needed" >&2
    exit 1
  fi
done < <("$toolchain/llvm-readelf" --needed-libs "$native_output/librackforge_android.so" \
  | sed -n 's/^[[:space:]]*\(lib[^[:space:]]*\.so\)[[:space:]]*$/\1/p')

# Builds predating generated assets copied this ignored artifact into the
# source tree. Gradle would merge both copies, so remove only the known legacy
# build output before creating the generated package set.
legacy_bundled_plugin="$android_project/app/src/main/assets/bundled-plugins/RackForge-Concert-Grand.rfplugin"
rm -f -- "$legacy_bundled_plugin"

bundled_output="$android_project/app/build/generated/bundled-plugins"
rm -rf -- "$bundled_output"
install -d "$bundled_output/bundled-plugins"
default_plugin="${RACKFORGE_BUNDLED_PLUGIN:-}"
if [[ "$edition" == standard ]]; then
  if [[ -z "$default_plugin" ]]; then
    # The Standard edition opens on the Concert Grand. It is built from this
    # repository rather than fetched, so a local build makes it when it is
    # missing -- skipping it silently shipped an APK without the piano.
    default_plugin="$repository/dist/bundled-plugins/RF-Concert-Grand.rfplugin"
    if [[ ! -f "$default_plugin" ]]; then
      (
        cd "$repository"
        rustup target add wasm32-unknown-unknown
        cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
        install -d "$(dirname "$default_plugin")"
        cargo run --release -p rackforge-store -- pack-wasm \
          plugins/concert-grand/package \
          target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm \
          "$default_plugin"
      )
    fi
  fi
  if [[ -n "$default_plugin" ]]; then
    [[ -f "$default_plugin" ]] || {
      printf 'RACKFORGE_BUNDLED_PLUGIN is not a file: %s\n' \
        "$default_plugin" >&2
      exit 2
    }
    install -m 0644 "$default_plugin" \
      "$bundled_output/bundled-plugins/$(basename "$default_plugin")"
  fi
  shopt -s nullglob
  for archive in "$official_plugins"/*.rfplugin; do
    install -m 0644 "$archive" "$bundled_output/bundled-plugins/$(basename "$archive")"
  done
  shopt -u nullglob
fi

web_output="$android_project/app/build/generated/web-ui/rackforge"
rm -rf -- "$web_output"
install -d "$web_output"
cp -R "$repository/web/dist/." "$web_output/"

cd "$android_project"
bash ./gradlew testDebugUnitTest assembleDebug --no-daemon

source_apk="$android_project/app/build/outputs/apk/debug/app-debug.apk"
[[ -f "$source_apk" ]] || {
  printf 'Gradle completed without producing %s.\n' "$source_apk" >&2
  exit 1
}
mkdir -p "$output_directory"
output_directory="$(cd "$output_directory" && pwd)"
install -m 0644 "$source_apk" "$output_directory/RackForge-debug.apk"
printf 'edition=%s\n' "$edition" >"$output_directory/build-info.txt"
printf 'RackForge Android (%s): %s\n' \
  "$edition" "$output_directory/RackForge-debug.apk"
