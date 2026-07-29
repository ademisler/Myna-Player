<p align="center">
  <img src="myna_player_icon.svg" alt="Myna Player" width="128" height="128">
</p>

# Myna Player

**Subtitles before the scene.**

Myna Player is a local-first AI media player that is being built to turn a video's audio into timed source text, detect the spoken language, translate the text through a selectable provider, and display the result as synchronized subtitles while the video is playing.

## Current milestone

Myna Player now has a native-player foundation instead of an HTML media surface:

- Tauri 2 + Leptos player shell with a native libVLC 3 `NSView` on macOS
- play/pause, seek, volume, mute, speed, fullscreen, audio and embedded subtitle tracks
- a focused player UI with auto-hiding controls, native drag/drop, and a scalable settings modal
- SQLite/WAL persistence for settings, playback position, processing checkpoints, source transcript segments, and provider-specific translations
- deterministic 30-second canonical windows, two-second extraction context, urgent seek generations, 90-second look-ahead, and background completion
- a pinned, statically built whisper.cpp Tauri sidecar that keeps the selected model loaded on a random loopback port
- SHA-256-verified Whisper and Silero VAD model management from the settings UI
- word-timestamp subtitle segmentation that preserves conversational context while displaying one timed cue at a time
- DeepL Free/Pro, OpenAI, Gemini, OpenRouter, and MiniMax adapters with strict cue-ID/timing preservation
- SRT/VTT export and a persistent subtitle text/timing correction editor
- native macOS `NSView` and Windows child-HWND video surfaces with staged libVLC runtimes
- signed macOS/notarized and Windows Authenticode release workflows (when repository signing secrets are configured)

Cloud translation is disabled by default. Source transcript and translated cues
are persisted as separate records and the render loop reads cached cues only.

## Architecture

```text
Native player clock
      │
      ▼
Persistent priority queue ──► FFmpeg audio window (16 kHz mono PCM)
                              │
                              ▼
                    long-lived local Whisper worker
                              │
                              ▼
                    language + timed source text
                              │
                              ▼
                   translation provider adapter
                              │
                              ▼
                   synchronized subtitle store
```

See [`docs/architecture.md`](docs/architecture.md) and [`docs/roadmap.md`](docs/roadmap.md).

## Development

Requirements:

- Rust stable
- `wasm32-unknown-unknown` target
- Trunk
- Tauri CLI 2
- FFmpeg and FFprobe
- whisper.cpp (`whisper-server`) only for development; packaged builds use the pinned Tauri sidecar
- models installed from Settings, or a multilingual GGML Whisper model supplied through `MYNA_PLAYER_WHISPER_MODEL`
- VLC 3 during development, or the staged libVLC bundle described below

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install tauri-cli --version '^2.0.0' --locked
cargo tauri dev
```

Checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
env -u NO_COLOR trunk build
```

## macOS libVLC packaging

libVLC binaries are never committed. The staging script downloads VLC 3.0.21
from VideoLAN, verifies the pinned architecture-specific SHA-256, and prepares
the dylibs, plugins, shared data, and license notice for Tauri:

```bash
scripts/package-macos.sh arm64
scripts/package-macos.sh x86_64
```

For a local packaging check using an already installed matching VLC build:

```bash
MYNA_PLAYER_VLC_APP=/Applications/VLC.app scripts/stage-libvlc-macos.sh arm64
env -u NO_COLOR cargo tauri build --config src-tauri/tauri.bundle.conf.json
```

Windows packaging is performed on a Windows host:

```powershell
./scripts/package-windows.ps1
```

The script verifies VLC 3.0.21, builds the pinned whisper.cpp sidecar, and produces the native Tauri installers. See [`docs/third-party-licenses.md`](docs/third-party-licenses.md) for runtime notices.

## License

GPL-3.0-or-later.
