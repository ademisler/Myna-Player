# SubAhead

**Subtitles before the scene.**

SubAhead is a local-first AI media player that is being built to turn a video's audio into timed source text, detect the spoken language, translate the text through a selectable provider, and display the result as synchronized subtitles while the video is playing.

## Current milestone

The repository now contains the first functional desktop foundation:

- Tauri 2 desktop shell with a Leptos interface
- local video file selection and playback
- FFprobe-based media and stream inspection
- FFmpeg extraction of 30-second, 16 kHz mono PCM windows
- look-ahead scheduler with urgent and normal processing windows
- real whisper.cpp transcription with language detection and timed segments
- optional DeepL Free/Pro translation with an in-memory API key
- automatic 90-second look-ahead processing while the video plays
- synchronized source and translated subtitle rendering
- runtime checks for FFmpeg, FFprobe, whisper.cpp, and the local model

The current MVP processes non-overlapping 30-second windows. Model management, persistent subtitle storage, overlap-aware stitching, and additional translation providers are the next milestones.

## Architecture

```text
Video player time
      │
      ▼
Look-ahead scheduler ──► FFmpeg audio window (16 kHz mono PCM)
                              │
                              ▼
                      ASR provider (Whisper)
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
- whisper.cpp (`whisper-cli`)
- a multilingual GGML Whisper model at the platform model path (`~/Library/Application Support/com.subahead.desktop/models/ggml-base.bin` on macOS) or `SUBAHEAD_WHISPER_MODEL`

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install tauri-cli --version '^2.0.0' --locked
cargo tauri dev
```

Checks:

```bash
cargo fmt --all -- --check
cargo test -p subahead-core
cargo check -p subahead-media -p subahead-pipeline -p subahead
trunk build
```

## License

GPL-3.0-or-later.
