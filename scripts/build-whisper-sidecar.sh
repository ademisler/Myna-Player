#!/usr/bin/env bash
set -euo pipefail

version="v1.9.1"
commit="f049fff95a089aa9969deb009cdd4892b3e74916"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
work_dir="${MYNA_PLAYER_WHISPER_BUILD_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/myna-player-whisper-$target}"
source_dir="$work_dir/source"
build_dir="$work_dir/build"
output_dir="$repo_dir/src-tauri/binaries"
extension=""
[[ "$target" == *windows* ]] && extension=".exe"
destination="$output_dir/whisper-server-$target$extension"
manifest="$repo_dir/src-tauri/vendor/whisper/BUILD_INFO"

if [[ "${MYNA_PLAYER_FORCE_RUNTIME_REBUILD:-0}" != "1" ]]   && [[ -x "$destination" ]]   && [[ -s "$manifest" ]]   && grep -Fqx "$commit" "$manifest"   && "$destination" --help >/dev/null 2>&1; then
  printf 'Reusing pinned whisper.cpp %s (%s) sidecar: %s
' "$version" "$commit" "$destination"
  exit 0
fi

rm -rf "$work_dir"
mkdir -p "$work_dir" "$output_dir"
git clone --filter=blob:none --no-checkout https://github.com/ggml-org/whisper.cpp.git "$source_dir"
git -C "$source_dir" checkout --detach "$commit"
actual="$(git -C "$source_dir" rev-parse HEAD)"
[[ "$actual" == "$commit" ]] || { echo "Unexpected whisper.cpp commit: $actual" >&2; exit 3; }

cmake_arch_args=()
if [[ "$target" == "aarch64-apple-darwin" ]]; then
  cmake_arch_args+=("-DCMAKE_OSX_ARCHITECTURES=arm64")
elif [[ "$target" == "x86_64-apple-darwin" ]]; then
  cmake_arch_args+=("-DCMAKE_OSX_ARCHITECTURES=x86_64")
fi

cmake -S "$source_dir" -B "$build_dir" \
  "${cmake_arch_args[@]}" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DWHISPER_BUILD_TESTS=OFF \
  -DWHISPER_BUILD_EXAMPLES=ON \
  -DWHISPER_BUILD_SERVER=ON \
  -DGGML_NATIVE=OFF \
  -DGGML_BACKEND_DL=OFF \
  -DGGML_BLAS=OFF \
  -DGGML_OPENMP=OFF
cmake --build "$build_dir" --config Release --target whisper-server --parallel

binary="$(find "$build_dir" -type f \( -name whisper-server -o -name whisper-server.exe \) | head -n 1)"
[[ -n "$binary" && -f "$binary" ]] || { echo "whisper-server build output not found" >&2; exit 4; }
cp "$binary" "$destination"
chmod 755 "$destination" 2>/dev/null || true
mkdir -p "$repo_dir/src-tauri/vendor/whisper"
cp "$source_dir/LICENSE" "$repo_dir/src-tauri/vendor/whisper/LICENSE.txt"
printf '%s\n' "$commit" > "$manifest"
printf 'Built pinned whisper.cpp %s (%s) sidecar: %s\n' "$version" "$commit" "$destination"
