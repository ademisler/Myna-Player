# Roadmap

## Milestone 1 — Desktop and media foundation

- [x] Tauri + Leptos application
- [x] native libVLC 3 player contract and macOS `NSView`
- [x] local selection and native drag/drop
- [x] transport, seek, volume, rate, fullscreen, audio/subtitle tracks
- [x] FFprobe metadata and stream discovery
- [x] FFmpeg PCM window extraction
- [x] deterministic priority scheduler with seek generations and EOF behavior
- [x] SQLite/WAL checkpoints and resume
- [x] ASR and translation provider interfaces

## Milestone 2 — Local speech recognition

- [x] long-lived whisper.cpp server worker
- [x] pinned optional `whisper-rs` Metal adapter
- [x] bundle the worker as a signed Tauri sidecar
- [x] model download and integrity verification
- [x] VAD-backed speech region detection
- [x] word/segment timestamp parsing
- [x] language detection confidence
- [x] cancellation after seek

## Milestone 3 — Translation

- [x] DeepL adapter
- [x] OpenAI/OpenRouter adapter
- [x] Gemini and MiniMax adapters
- [x] context-aware batch translation
- [x] explicit source-text-before-translation workflow
- [x] bounded retry and rate-limit states
- [x] provider-specific translation persistence

## Milestone 4 — Subtitle experience

- [x] source, translated, and dual subtitle modes
- [x] SQLite subtitle cache
- [x] settings modal with scalable left navigation
- [x] smart start and explicit Play now
- [x] SRT/VTT export
- [x] editing and correction
- [x] seek-aware queue invalidation
- [x] signed ARM64 and Intel release automation
- [x] Windows child HWND runtime and packaging
