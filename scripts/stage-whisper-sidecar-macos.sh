#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This staging helper is macOS-only." >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
source_binary="${MYNA_PLAYER_WHISPER_SERVER:-$(command -v whisper-server || true)}"
if [[ -z "$source_binary" || ! -x "$source_binary" ]]; then
  echo "whisper-server is not installed. Install whisper-cpp or set MYNA_PLAYER_WHISPER_SERVER." >&2
  exit 3
fi

binary_dir="$repo_dir/src-tauri/binaries"
library_dir="$repo_dir/src-tauri/vendor/whisper/lib"
mkdir -p "$binary_dir" "$library_dir"
find "$library_dir" -mindepth 1 -delete
sidecar="$binary_dir/whisper-server-$target"
cp "$source_binary" "$sidecar"
chmod 755 "$sidecar"

resolve_rpath_dependency() {
  local dependency="$1"
  local binary="$2"
  if [[ "$dependency" != @rpath/* ]]; then
    printf '%s\n' "$dependency"
    return
  fi
  local name="${dependency#@rpath/}"
  local candidate
  for candidate in \
    "$(brew --prefix whisper-cpp 2>/dev/null || true)/lib/$name" \
    "$(brew --prefix ggml 2>/dev/null || true)/lib/$name" \
    "$(brew --prefix libomp 2>/dev/null || true)/lib/$name"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
  echo "Could not resolve $dependency required by $binary" >&2
  exit 4
}

queue=("$sidecar")
processed=()
while ((${#queue[@]})); do
  current="${queue[0]}"
  queue=("${queue[@]:1}")
  if printf '%s\n' "${processed[@]:-}" | grep -Fxq "$current"; then
    continue
  fi
  processed+=("$current")
  while IFS= read -r dependency; do
    [[ -z "$dependency" ]] && continue
    case "$dependency" in
      /usr/lib/*|/System/*) continue ;;
    esac
    resolved="$(resolve_rpath_dependency "$dependency" "$current")"
    basename="$(basename "$dependency")"
    destination="$library_dir/$basename"
    if [[ ! -f "$destination" ]]; then
      cp -L "$resolved" "$destination"
      chmod 755 "$destination"
      queue+=("$destination")
    fi
    install_name_tool -change "$dependency" "@rpath/$basename" "$current" 2>/dev/null || true
  done < <(otool -L "$current" | tail -n +2 | awk '{print $1}')
done

for library in "$library_dir"/*.dylib; do
  [[ -f "$library" ]] || continue
  install_name_tool -id "@rpath/$(basename "$library")" "$library" 2>/dev/null || true
  install_name_tool -add_rpath "@loader_path" "$library" 2>/dev/null || true
done
install_name_tool -add_rpath "@executable_path/../Resources/whisper/lib" "$sidecar" 2>/dev/null || true

# Ad-hoc signing makes the local debug sidecar acceptable to hardened macOS. Release CI
# replaces this with the configured Developer ID identity when signing the full app bundle.
codesign --force --sign - "$sidecar"
for library in "$library_dir"/*.dylib; do
  [[ -f "$library" ]] && codesign --force --sign - "$library"
done

DYLD_LIBRARY_PATH="$library_dir" "$sidecar" --help >/dev/null 2>&1 || {
  echo "Staged whisper-server failed its smoke test." >&2
  exit 5
}
printf 'Staged whisper-server sidecar for %s at %s\n' "$target" "$sidecar"
