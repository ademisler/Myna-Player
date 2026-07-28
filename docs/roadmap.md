# Roadmap

## Milestone 1 — Desktop and media foundation

- [x] Tauri + Leptos application
- [x] local video selection and playback
- [x] FFprobe metadata and stream discovery
- [x] FFmpeg PCM window extraction
- [x] deterministic look-ahead scheduler
- [x] ASR and translation provider interfaces

## Milestone 2 — Local speech recognition

- [ ] managed whisper.cpp installation
- [ ] model download and integrity verification
- [ ] VAD-backed speech region detection
- [ ] word/segment timestamp parsing
- [ ] language detection confidence
- [ ] cancellation after seek

## Milestone 3 — Translation

- [x] DeepL adapter
- [ ] OpenAI-compatible adapter
- [ ] Ollama/local adapter
- [x] context-aware batch translation
- [ ] retry, rate-limit, and offline states

## Milestone 4 — Subtitle experience

- [ ] source, translated, and dual subtitle modes
- [ ] SQLite subtitle cache
- [ ] SRT/VTT export
- [ ] editing and correction
- [ ] seek-aware queue invalidation
- [ ] full media-player integration beyond the initial HTML media surface
