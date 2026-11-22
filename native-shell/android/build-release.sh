#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo"

# The Node step bundles the scripts used in the YouTube and token WebViews.
npm --prefix native-shell run page:build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/native-shell/app/target}"
cargo build --lib --locked --release --manifest-path native-shell/app/Cargo.toml --target aarch64-linux-android

inputs="$repo/native-shell/android/build/govue"
mkdir -p "$inputs/jniLibs/arm64-v8a"
"$(dirname -- "$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER")/llvm-strip" --strip-unneeded \
  -o "$inputs/jniLibs/arm64-v8a/libnsh.so" "$CARGO_TARGET_DIR/aarch64-linux-android/release/libnsh.so"
install -D -m 644 assets/icon.png "$inputs/res/drawable-nodpi/govue_icon.png"
"${GRADLE:-gradle}" -p native-shell/android assembleRelease "$@"
