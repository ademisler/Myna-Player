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
- provider contracts for Whisper-compatible ASR and translation engines
- runtime checks for FFmpeg, FFprobe, and whisper.cpp

Whisper transcription and provider-backed translation are the next implementation milestone. The UI explicitly reports them as not configured rather than simulating output.

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
