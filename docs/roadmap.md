# Delivery status

Status meanings:

- **Implemented**: code exists and is covered by deterministic unit tests.
- **Integrated**: the feature is exercised through the real native runtime on a development machine.
- **Packaged**: every required runtime and license is included in the installer.
- **Release verified**: a signed artifact has passed a clean GitHub-hosted runner.

## Desktop and media foundation

| Capability | Status |
| --- | --- |
| Tauri 2 + Leptos desktop shell | Integrated |
| macOS libVLC `NSView` surface | Integrated |
| Windows libVLC child HWND surface | Implemented; clean-runner workflow added |
| Transport, seek, replay, volume, speed and fullscreen | Integrated |
| Metadata and stream discovery | Integrated |
| Language/title-aware audio stream mapping | Implemented and tested |
| Bundled FFmpeg and FFprobe sidecars | Packaged on macOS arm64; clean macOS/Windows workflows added |
| Deterministic seek/look-ahead scheduler | Integrated |

## Speech and subtitle pipeline

| Capability | Status |
| --- | --- |
| Pinned whisper.cpp sidecar | Integrated and packaged |
| Optional native Whisper/Metal feature | Compiles with `--all-features` |
| SHA-256 model installation | Integrated |
| Download locking and byte progress | Implemented and tested |
| Silero VAD | Integrated |
| Word timestamp cue segmentation | Integrated |
| Dynamic cache fingerprint for model/language/VAD/chunk changes | Implemented and tested |
| Independent ASR and cloud translation workers | Implemented and tested |
| Incremental cue patches over Tauri Channels | Implemented and tested |
| 100 ms player clock and binary-search cue lookup | Implemented and tested |

## Data, privacy and operations

| Capability | Status |
| --- | --- |
| SQLite/WAL checkpoints | Integrated |
| Transactional v1 to v2 migration | Implemented and tested |
| Ephemeral transcript deletion | Implemented and tested |
| Cache size enforcement | Implemented and tested |
| Translation invalidation after source edits | Implemented and tested |
| SRT/VTT export and cue correction | Integrated |
| Credential storage in the operating-system keychain | Integrated |
| Bounded worker diagnostics | Implemented and exposed in Settings |
| Strict CSP and least-privilege capabilities | Implemented |
| Rust advisory/license/source policy | Enforced by CI |

## Release verification

The following workflows are the source of truth:

- `quality.yml`: formatting, all-feature tests, Clippy, Leptos build,
  dependency policy and Windows compile checks on pull requests and `main`.
- `native-smoke.yml`: builds standalone macOS and Windows packages on clean
  GitHub-hosted runners, checks bundled runtimes/licenses, and exercises libVLC
  replay on macOS.
- `release.yml`: requires signing secrets, signs nested runtimes, notarizes
  macOS artifacts, verifies Authenticode signatures, and publishes SHA-256
  checksums.

A release is not marked **Release verified** until the corresponding workflow
has completed successfully with the repository signing secrets configured.
