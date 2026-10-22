#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Avoid the workstation's hardware WebGL context loss during 3D capture.
export PRODUCER_BROWSER_GPU_MODE="${PRODUCER_BROWSER_GPU_MODE:-software}"

# Use existing host tools; Nix supplies these two on the workstation.
if ! command -v ffmpeg >/dev/null || ! command -v chromium >/dev/null; then
  if [[ "${GOVUE_VIDEO_NIX:-0}" != 1 ]] && command -v nix >/dev/null; then
    exec nix shell nixpkgs#ffmpeg nixpkgs#chromium \
      -c env GOVUE_VIDEO_NIX=1 bash scripts/hf.sh "$@"
  fi
fi
if command -v chromium >/dev/null; then
  export HYPERFRAMES_BROWSER_PATH="$(command -v chromium)"
fi
if command -v ffmpeg >/dev/null; then
  export HYPERFRAMES_FFMPEG_PATH="$(command -v ffmpeg)"
  export HYPERFRAMES_FFPROBE_PATH="$(command -v ffprobe)"
fi
exec node node_modules/hyperframes/bin/hyperframes.mjs "$@"
