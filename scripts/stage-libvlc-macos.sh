#!/usr/bin/env bash
set -euo pipefail

version="3.0.21"
requested_arch="${1:-$(uname -m)}"

case "$requested_arch" in
  arm64|aarch64)
    vlc_arch="arm64"
    expected_sha256="15dd65bf6489da9ec6a67f5585c74c40a58993acff41a82958a916dd74178044"
    ;;
  x86_64|intel64)
    vlc_arch="intel64"
    expected_sha256="d431fd051c3dc7af02bd313c6d05d90cf604b70ed3ec5bba6fd4c49ef3e638d9"
    ;;
  *)
    echo "Unsupported macOS architecture: $requested_arch" >&2
    exit 2
    ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
stage_dir="$repo_dir/src-tauri/vendor/libvlc"
cache_dir="${MYNA_PLAYER_LIBVLC_CACHE:-${TMPDIR:-/tmp}/myna-player-libvlc-cache}"
image_name="vlc-${version}-${vlc_arch}.dmg"
image_path="$cache_dir/$image_name"
download_base_url="${MYNA_PLAYER_LIBVLC_BASE_URL:-https://get.videolan.org/vlc/${version}/macosx}"
download_url="${download_base_url%/}/${image_name}"
mount_dir=""
vlc_app="${MYNA_PLAYER_VLC_APP:-}"

cleanup() {
  if [[ -n "$mount_dir" ]]; then
    hdiutil detach "$mount_dir" -quiet || true
  fi
  if [[ -n "$mount_dir" ]] && [[ "$mount_dir" == "${TMPDIR:-/tmp}/"* ]]; then
    rmdir "$mount_dir" 2>/dev/null || true
  fi
}
trap cleanup EXIT

if [[ -n "$vlc_app" ]]; then
  if [[ ! -d "$vlc_app" ]]; then
    echo "MYNA_PLAYER_VLC_APP does not point to a VLC.app bundle: $vlc_app" >&2
    exit 3
  fi
else
  mkdir -p "$cache_dir"
  if [[ -f "$image_path" ]]; then
    cached_sha256="$(shasum -a 256 "$image_path" | awk '{print $1}')"
    if [[ "$cached_sha256" != "$expected_sha256" ]]; then
      quarantine_path="${image_path}.invalid-$(date +%Y%m%d%H%M%S)"
      mv "$image_path" "$quarantine_path"
      echo "Quarantined invalid cached image at $quarantine_path" >&2
    fi
  fi
  if [[ ! -f "$image_path" ]]; then
    partial_image="$image_path.part"
    curl --fail --location --continue-at - --progress-bar \
      "$download_url" --output "$partial_image"
    mv "$partial_image" "$image_path"
  fi

  actual_sha256="$(shasum -a 256 "$image_path" | awk '{print $1}')"
  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    echo "Checksum mismatch for $image_path" >&2
    echo "Expected: $expected_sha256" >&2
    echo "Actual:   $actual_sha256" >&2
    exit 3
  fi

  mount_dir="$(mktemp -d "${TMPDIR:-/tmp}/myna-player-vlc.XXXXXX")"
  hdiutil attach "$image_path" -nobrowse -readonly -mountpoint "$mount_dir" -quiet
  vlc_app="$mount_dir/VLC.app"
fi

vlc_runtime="$vlc_app/Contents/MacOS"
if [[ ! -d "$vlc_runtime/plugins" ]] || [[ ! -f "$vlc_runtime/lib/libvlc.5.dylib" ]]; then
  echo "The verified VLC image does not contain the expected libVLC runtime." >&2
  exit 4
fi
library_arches="$(lipo -archs "$vlc_runtime/lib/libvlc.5.dylib")"
expected_macho_arch="$vlc_arch"
if [[ "$vlc_arch" == "intel64" ]]; then
  expected_macho_arch="x86_64"
fi
if [[ " $library_arches " != *" $expected_macho_arch "* ]]; then
  echo "VLC runtime architecture mismatch: expected $expected_macho_arch, found $library_arches" >&2
  exit 5
fi

if [[ -e "$stage_dir" ]]; then
  find "$stage_dir" -mindepth 1 -delete
fi
mkdir -p "$stage_dir/lib"
cp -L "$vlc_runtime/lib/libvlc.5.dylib" "$stage_dir/lib/libvlc.dylib"
cp -L "$vlc_runtime/lib/libvlccore.9.dylib" "$stage_dir/lib/libvlccore.dylib"
ditto "$vlc_runtime/plugins" "$stage_dir/plugins"
ditto "$vlc_runtime/share" "$stage_dir/share"
curl --fail --location --silent --show-error \
  "https://raw.githubusercontent.com/videolan/vlc/${version}/COPYING" \
  --output "$stage_dir/VLC-COPYING.txt"

printf 'Staged libVLC %s (%s) in %s\n' "$version" "$vlc_arch" "$stage_dir"
