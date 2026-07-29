<p align="center">
  <a href="https://myna-player.github.io/">
    <img src="myna_player_icon.svg" alt="Myna Player" width="132" height="132">
  </a>
</p>

<h1 align="center">Myna Player</h1>

<p align="center"><strong>Watch in any language.</strong></p>

<p align="center">
  A local-first, open-source AI video player that transcribes ahead, translates with context, and displays each subtitle at the right moment.
</p>

<p align="center">
  <a href="https://myna-player.github.io/">Website</a> ·
  <a href="docs/architecture.md">Architecture</a> ·
  <a href="docs/roadmap.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="SECURITY.md">Security</a>
</p>

<p align="center">
  <a href="https://github.com/ademisler/Myna-Player/actions/workflows/quality.yml"><img alt="Quality" src="https://github.com/ademisler/Myna-Player/actions/workflows/quality.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: GPL-3.0-or-later" src="https://img.shields.io/badge/license-GPL--3.0--or--later-f0b51b"></a>
  <img alt="Status: public alpha" src="https://img.shields.io/badge/status-public%20alpha-171918">
  <img alt="Rust and Tauri" src="https://img.shields.io/badge/Rust%20%2B%20Tauri-2b2d2b">
</p>

## Project status

Myna Player is a **public alpha**. The core macOS application works, Windows code and packaging are under active verification, and the first stable public release has not been published yet. Expect interface, storage, and packaging changes before 1.0.

The repository is open for review and contribution. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing substantial changes.

## Why Myna Player exists

Live subtitle systems often choose between speed and coherence. Translating tiny fragments quickly loses context; translating long blocks makes subtitles arrive late. Myna Player separates those concerns:

1. it processes audio ahead of playback;
2. Whisper produces word-level timing locally;
3. natural subtitle cues are derived from those timestamps;
4. translation sees surrounding context but may not rewrite cue IDs or timing;
5. cached cues are rendered against the native player clock.

## Current capabilities

- Native libVLC video surface on macOS and a Windows child-HWND implementation.
- Play, pause, seek, volume, mute, speed, fullscreen, audio tracks, and embedded subtitle tracks.
- Pinned FFmpeg, FFprobe, whisper.cpp, libVLC, and model packaging workflows.
- Local Whisper transcription with word timestamps and optional Silero VAD.
- Resumable, prioritized look-ahead processing with SQLite/WAL checkpoints.
- Context-aware DeepL, Gemini, MiniMax, OpenAI, and OpenRouter adapters.
- Strict translation validation that preserves cue IDs and source timing.
- Source, translated, and dual SRT/VTT export.
- Persistent subtitle text/timing corrections.
- Per-video reset for deleting transcripts, translations, checkpoints, cache, and playback position.
- Model download, SHA-256 verification, runtime diagnostics, and bounded worker logs.

## Privacy model

Local transcription is the default. FFmpeg, VAD, and Whisper run on the user's computer. Cloud translation is opt-in and receives finalized text with limited neighboring context—not the video or extracted audio.

Credentials use the operating system credential store. Generated subtitle data is stored locally and can be deleted per video with **Reset this video**.

See [docs/privacy.md](docs/privacy.md) and [SECURITY.md](SECURITY.md).

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
                  timed source subtitle segments
                              │
                    ┌─────────┴─────────┐
                    ▼                   ▼
             local subtitle cache   optional translator
                    │                   │
                    └─────────┬─────────┘
                              ▼
                  synchronized subtitle renderer
```

The workspace separates core models, media probing/extraction, scheduling, player integration, provider adapters, pipeline processing, and storage. See [docs/architecture.md](docs/architecture.md).

## Building from source

### Requirements

- Rust stable with the `wasm32-unknown-unknown` target
- Trunk `0.21.14`
- Tauri CLI `2.11.4`
- Platform-native build prerequisites for Tauri
- VLC 3, FFmpeg/FFprobe, and whisper.cpp for unbundled development, or the staging scripts for packaged runtimes

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version 2.11.4 --locked
cargo tauri dev
```

Models can be installed from **Settings → Transcription**, or a multilingual GGML Whisper model can be supplied through `MYNA_PLAYER_WHISPER_MODEL`.

### Quality checks

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check licenses bans sources advisories
trunk build --release
git diff --check
```

## Packaging

Runtime binaries and models are not committed to Git. Packaging scripts download or build pinned components, verify checksums, stage required licenses, and produce native Tauri bundles.

```bash
# macOS
scripts/package-macos.sh arm64
scripts/package-macos.sh x86_64

# Windows PowerShell
./scripts/package-windows.ps1
```

Signing and notarization are performed by the release workflow when repository signing secrets are configured. Runtime and model licensing notes are documented in [docs/third-party-licenses.md](docs/third-party-licenses.md).

## Contributing and community

- Read [CONTRIBUTING.md](CONTRIBUTING.md).
- Use GitHub Discussions for questions and early ideas.
- Use issue forms for reproducible bugs and focused feature proposals.
- Report vulnerabilities privately according to [SECURITY.md](SECURITY.md).
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Myna Player is free software licensed under **GPL-3.0-or-later**. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).
