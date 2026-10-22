#!/usr/bin/env bash
set -euo pipefail

# Use the same software WebGL path as the validated snapshots and draft.
# An isolated profile prevents an existing hardware-GPU browser from absorbing
# the launch and silently ignoring these flags.
cd "$(dirname "$0")/.."
browser="${HYPERFRAMES_REVIEW_BROWSER:-brave}"
if ! command -v "$browser" >/dev/null; then
  browser="${HYPERFRAMES_BROWSER_PATH:-chromium}"
fi
exec "$browser" \
  --user-data-dir="$PWD/.hyperframes/review-browser-profile" \
  --use-gl=angle \
  --use-angle=swiftshader \
  --enable-unsafe-swiftshader \
  --enable-webgl \
  --disable-gpu-compositing \
  --autoplay-policy=no-user-gesture-required \
  --disable-background-media-suspend \
  --no-first-run \
  --no-default-browser-check \
  "${@:-http://localhost:3077/#project/govue-launch}"
