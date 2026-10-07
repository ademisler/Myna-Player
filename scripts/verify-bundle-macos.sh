#!/usr/bin/env bash
set -euo pipefail

app="${1:?usage: verify-bundle-macos.sh /path/to/Myna Player.app}"
[[ -d "$app" ]] || { echo "App bundle is missing: $app" >&2; exit 2; }
macos="$app/Contents/MacOS"
resources="$app/Contents/Resources"
frameworks="$app/Contents/Frameworks"

find_executable() {
  local prefix="$1"
  find "$macos" -maxdepth 1 -type f -name "$prefix*" -perm -111 | head -n 1
}

ffmpeg="$(find_executable ffmpeg)"
ffprobe="$(find_executable ffprobe)"
whisper="$(find_executable whisper-server)"
[[ -n "$ffmpeg" && -n "$ffprobe" && -n "$whisper" ]]
[[ -f "$frameworks/libvlc.dylib" && -f "$frameworks/libvlccore.dylib" ]]
[[ -d "$resources/vlc/plugins" ]]
for license in \
  Myna-Player-GPL-3.0.txt \
  Myna-Player-Third-Party-Notices.md \
  VLC-COPYING.txt \
  FFmpeg-COPYING.LGPLv2.1.txt \
  FFmpeg-LICENSE.md \
  whisper.cpp-LICENSE.txt; do
  [[ -s "$resources/licenses/$license" ]] || { echo "Missing license: $license" >&2; exit 3; }
done
"$ffmpeg" -version >/dev/null
"$ffprobe" -version >/dev/null
"$whisper" --help >/dev/null 2>&1
file "$ffmpeg" "$ffprobe" "$whisper" "$frameworks/libvlc.dylib"
printf 'Verified bundle runtimes and licenses: %s\n' "$app"
