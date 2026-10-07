#!/usr/bin/env bash
set -euo pipefail

requested_arch="${1:-$(uname -m)}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"

case "$requested_arch" in
  arm64|aarch64)
    rust_target="aarch64-apple-darwin"
    ;;
  x86_64|intel64)
    rust_target="x86_64-apple-darwin"
    ;;
  *)
    echo "Unsupported macOS architecture: $requested_arch" >&2
    exit 2
    ;;
esac

"$script_dir/stage-libvlc-macos.sh" "$requested_arch"
"$script_dir/build-whisper-sidecar.sh" "$rust_target"
"$script_dir/build-ffmpeg-sidecars.sh" "$rust_target"
cd "$repo_dir"
env -u NO_COLOR cargo tauri build \
  --target "$rust_target" \
  --bundles "${MYNA_PLAYER_BUNDLES:-app}" \
  --config "src-tauri/tauri.bundle.conf.json"

app_bundle="$repo_dir/target/$rust_target/release/bundle/macos/Myna Player.app"
if [[ -d "$app_bundle" && -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --deep --sign - "$app_bundle"
  codesign --verify --deep --strict --verbose=2 "$app_bundle"
  printf 'Applied and verified local ad-hoc signature: %s\n' "$app_bundle"
fi
