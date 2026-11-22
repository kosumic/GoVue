#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo"

# The Node step bundles the scripts used in the YouTube and token WebViews.
npm --prefix native-shell run page:build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/native-shell/app/target}"
export GOVUE_APP_ID=io.github.kaminarios.govue
if [[ "${GOVUE_PREVIEW:-0}" == 1 ]]; then
  GOVUE_APP_ID=io.github.kaminarios.govue.nsh
fi
cargo build --lib --locked --release --manifest-path native-shell/app/Cargo.toml --target aarch64-linux-android
if [[ -n "${NSH_SOURCE:-}" ]]; then
  GOVUE_NSH_ANDROID_JAVA="$NSH_SOURCE/crates/nsh-platform-android/java"
else
  GOVUE_NSH_ANDROID_JAVA=$(cargo metadata --offline --locked --format-version 1 \
    --manifest-path native-shell/app/Cargo.toml --filter-platform aarch64-linux-android |
    node --input-type=module -e '
      import { readFileSync } from "node:fs";
      import { dirname, join } from "node:path";
      const packages = JSON.parse(readFileSync(0, "utf8")).packages;
      const android = packages.find((entry) => entry.name === "nsh-platform-android");
      if (!android) throw Error("The locked Native Shell Android package is unavailable.");
      console.log(join(dirname(android.manifest_path), "java"));
    ')
fi
export GOVUE_NSH_ANDROID_JAVA

inputs="$repo/native-shell/android/build/govue"
mkdir -p "$inputs/jniLibs/arm64-v8a"
"$(dirname -- "$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER")/llvm-strip" --strip-unneeded \
  -o "$inputs/jniLibs/arm64-v8a/libnsh.so" "$CARGO_TARGET_DIR/aarch64-linux-android/release/libnsh.so"
install -D -m 644 assets/icon.png "$inputs/res/drawable-nodpi/govue_icon.png"
"${GRADLE:-gradle}" -p native-shell/android assembleRelease "$@"
