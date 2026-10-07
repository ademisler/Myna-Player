#!/usr/bin/env bash
set -euo pipefail

version="7.1.1"
expected_sha256="733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
cache_dir="${MYNA_PLAYER_FFMPEG_CACHE:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/myna-player-ffmpeg-cache}"
work_dir="${MYNA_PLAYER_FFMPEG_BUILD_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/myna-player-ffmpeg-$target}"
archive="$cache_dir/ffmpeg-$version.tar.xz"
source_dir="$work_dir/source"
build_dir="$work_dir/build"
output_dir="$repo_dir/src-tauri/binaries"
url="https://ffmpeg.org/releases/ffmpeg-$version.tar.xz"

mkdir -p "$cache_dir" "$output_dir"

compute_sha256() {
  local path="$1"
  local digest
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$path" | awk '{print $1}')"
  else
    digest="$(shasum -a 256 "$path" | awk '{print $1}')"
  fi
  # GNU checksum tools prefix the whole line with a backslash when the filename
  # needs escaping. MSYS2 can also leave carriage returns in captured output.
  # A SHA-256 digest begins with a hexadecimal character, so discard only the
  # leading non-hex transport markers and normalize the remaining digest.
  printf '%s' "$digest" \
    | tr -d '\r' \
    | sed 's/^[^0-9A-Fa-f]*//' \
    | tr '[:upper:]' '[:lower:]'
}

download_archive() {
  rm -f "$archive.part"
  curl --fail --location --retry 5 --retry-delay 2 --retry-all-errors \
    --continue-at - --progress-bar "$url" --output "$archive.part"
  mv "$archive.part" "$archive"
}

if [[ ! -f "$archive" ]]; then
  download_archive
fi
actual_sha256="$(compute_sha256 "$archive")"
if [[ "$actual_sha256" != "$expected_sha256" ]]; then
  echo "Cached FFmpeg archive failed verification; downloading a clean copy." >&2
  rm -f "$archive"
  download_archive
  actual_sha256="$(compute_sha256 "$archive")"
fi
[[ "$actual_sha256" == "$expected_sha256" ]] || {
  echo "FFmpeg checksum mismatch. Expected $expected_sha256, got $actual_sha256" >&2
  exit 3
}

extension=""
[[ "$target" == *windows* ]] && extension=".exe"
ffmpeg_output="$output_dir/ffmpeg-$target$extension"
ffprobe_output="$output_dir/ffprobe-$target$extension"
license_dir="$repo_dir/src-tauri/vendor/ffmpeg"
if [[ "${MYNA_PLAYER_FORCE_RUNTIME_REBUILD:-0}" != "1" ]]   && [[ -x "$ffmpeg_output" && -x "$ffprobe_output" ]]   && "$ffmpeg_output" -version 2>/dev/null | head -n 1 | grep -Fq "ffmpeg version $version"   && "$ffprobe_output" -version 2>/dev/null | head -n 1 | grep -Fq "ffprobe version $version"   && [[ -s "$license_dir/COPYING.LGPLv2.1.txt" && -s "$license_dir/LICENSE.md" ]]; then
  printf 'Reusing verified FFmpeg %s sidecars for %s.
' "$version" "$target"
  exit 0
fi

rm -rf "$work_dir"
mkdir -p "$source_dir" "$build_dir"
tar -xf "$archive" -C "$source_dir" --strip-components=1

extra_args=()
case "$target" in
  aarch64-apple-darwin)
    extra_args+=(--arch=arm64 --cc=clang --extra-cflags=-arch\ arm64 --extra-ldflags=-arch\ arm64)
    ;;
  x86_64-apple-darwin)
    extra_args+=(--arch=x86_64 --cc=clang --extra-cflags=-arch\ x86_64 --extra-ldflags=-arch\ x86_64)
    ;;
  x86_64-pc-windows-msvc)
    # This script is executed from an MSYS2 MINGW64 shell in Windows CI. The
    # resulting standalone PE files are valid Tauri sidecars regardless of the
    # Rust target triple used to name them.
    extra_args+=(--arch=x86_64 --target-os=mingw32)
    ;;
  *)
    echo "Unsupported FFmpeg sidecar target: $target" >&2
    exit 2
    ;;
esac

pushd "$build_dir" >/dev/null
"$source_dir/configure" \
  --prefix="$build_dir/install" \
  --disable-shared \
  --enable-static \
  --disable-doc \
  --disable-debug \
  --disable-ffplay \
  --disable-autodetect \
  --disable-network \
  --enable-small \
  "${extra_args[@]}"
make -j"${MYNA_PLAYER_BUILD_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)}" ffmpeg ffprobe
popd >/dev/null

for program in ffmpeg ffprobe; do
  source_binary="$build_dir/$program$extension"
  destination="$output_dir/$program-$target$extension"
  [[ -x "$source_binary" || -f "$source_binary" ]] || { echo "$program build output missing" >&2; exit 4; }
  cp "$source_binary" "$destination"
  chmod 755 "$destination" 2>/dev/null || true
  "$destination" -version >/dev/null 2>&1 || { echo "$destination failed smoke test" >&2; exit 5; }
done

mkdir -p "$license_dir"
cp "$source_dir/COPYING.LGPLv2.1" "$license_dir/COPYING.LGPLv2.1.txt"
cp "$source_dir/LICENSE.md" "$license_dir/LICENSE.md"
printf 'Built verified FFmpeg %s sidecars for %s\n' "$version" "$target"
