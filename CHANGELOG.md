# Changelog

All notable changes to Myna Player will be documented here.

The project follows the principles of [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and intends to use semantic versioning after the first public release.

## [Unreleased]

### Added

- Public open-source project governance and contribution documentation.
- Per-video reset action for deleting transcripts, translations, processing checkpoints, cache records, and remembered playback position.
- Official project website at <https://myna-player.github.io/>.

### Changed

- Native package smoke verification now retries and validates the official Windows VLC archive and resolves bundled macOS libVLC dependencies from the app Frameworks directory.
- Production runtime hardening for bundled FFmpeg, FFprobe, whisper.cpp, libVLC, model verification, CSP, storage migrations, and provider validation.
- MiniMax M3 and Gemini 3.5 Flash provider defaults and stricter structured-output handling.

### Security

- Repository history and tracked files scanned for common credential and private-key patterns before public release.
