# Contributing to Myna Player

Thank you for helping build Myna Player. The project welcomes bug reports, documentation improvements, provider integrations, platform work, performance improvements, accessibility fixes, and focused product proposals.

## Before opening work

- Search existing issues and discussions first.
- Use an issue for changes that affect architecture, storage, the subtitle pipeline, provider behavior, or user-facing workflows.
- Keep pull requests focused. Unrelated refactors should be separate.
- Never include API keys, model files, media files, credentials, or private user data.

## Development setup

Myna Player uses Rust, Tauri 2, Leptos, Trunk, libVLC, FFmpeg/FFprobe, SQLite, and whisper.cpp.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version 2.11.4 --locked
cargo tauri dev
```

Development runs may use local VLC, FFmpeg, FFprobe, and whisper.cpp installations. Release bundles stage pinned runtimes through the scripts in `scripts/`.

## Required checks

Run these before submitting a pull request:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check licenses bans sources advisories
trunk build --release
git diff --check
```

Platform-specific changes should also run the relevant native smoke test or packaging script.

## Code expectations

- Prefer explicit errors over silent fallbacks.
- Preserve subtitle cue identifiers and source timing through translation.
- Treat media paths, transcripts, and credentials as sensitive data.
- Add regression tests for storage migrations, cache invalidation, timing, provider parsing, and native-player state transitions.
- Avoid network access unless the user explicitly selected a cloud provider or requested a model/runtime download.
- Keep UI controls keyboard accessible and usable at compact window sizes.

## Commit and pull request guidance

Use clear, imperative commit subjects, for example:

```text
Fix cache invalidation after model changes
Add Gemini response validation
```

A pull request should explain:

- the user problem;
- the approach taken;
- tests performed;
- privacy, security, migration, and platform implications;
- screenshots or recordings for visible UI changes.

## Licensing

By contributing, you agree that your contribution is licensed under the repository's [GPL-3.0-or-later license](LICENSE).
