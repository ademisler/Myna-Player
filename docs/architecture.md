# Architecture

## Product boundary

Myna Player is a media player with a just-in-time subtitle pipeline. It is not a generic transcription editor. For local files, analysis reads directly from the media file so processing can stay ahead of playback instead of waiting for audible system output.

## Workspace

- `myna-player-ui`: Leptos CSR interface compiled to WebAssembly.
- `src-tauri`: composition root, macOS native surface, ordered Channels, and narrow IPC commands.
- `myna-player-core`: serializable domain models and deterministic scheduling.
- `myna-player-media`: FFprobe inspection and FFmpeg audio extraction.
- `myna-player-player`: `PlayerEngine`, direct libVLC 3 bindings, safe wrapper, and unavailable adapter.
- `myna-player-jobs`: persistent-priority scheduling rules, generations, promotion, retry, and resume.
- `myna-player-storage`: versioned SQLite/WAL storage and media fingerprints.
- `myna-player-pipeline`: ASR/translation execution, persistent Whisper sidecar, VAD, timed cue segmentation, DeepL, OpenAI, Gemini, OpenRouter, and MiniMax adapters.
- `myna-player-providers`: provider registry and operating-system credential boundary.

## Processing contract

1. Fingerprint the local file and restore completed checkpoints for the selected audio track and pipeline version.
2. Schedule deterministic, non-overlapping 30-second canonical windows. The first window is urgent, the next 90 seconds are promoted, and the rest stay background priority.
3. On seek, cancel the active worker connection, increment the generation, preserve completed cache, and schedule the new region.
4. Extract two seconds of context around a canonical window as mono, 16 kHz, signed 16-bit PCM. Paths are passed as process arguments.
5. Run Silero VAD in the bundled whisper.cpp worker, transcribe only speech regions, and convert word timestamps into readable, non-overlapping subtitle cues.
6. If cloud translation was explicitly enabled, translate all finalized timed cues with shared dialogue context while preserving one output per cue ID and its source timing.
7. Persist source and provider/target-specific translations separately.
8. Publish ordered player/processing snapshots over Tauri Channels.
9. Render only cached cues against the native player clock; neither the WebView render loop nor libVLC calls ASR or translation.

## Native player composition

On macOS, libVLC renders into an application-owned `NSView` placed below the
transparent Tauri WebView. The UI layer owns controls and generated subtitle
cues. Surface geometry is synchronized on resize/fullscreen on the AppKit main
thread. A direct, limited C binding keeps all unsafe libVLC interaction inside
`myna-player-player`.

On Windows, Myna Player creates an application-owned child HWND below WebView2 and binds libVLC to that surface. The verified VLC runtime, plugins, and statically built whisper.cpp sidecar are staged into the Tauri bundle by the Windows packaging scripts.

## Safety and privacy

- Local file paths stay in the native backend.
- Media processing commands use argument arrays rather than shell interpolation.
- Cloud translation must be opt-in and clearly label which text leaves the device.
- API keys are stored through the operating system credential store under `translation/{provider_id}` and are never returned to the UI.
- The bundled Whisper HTTP sidecar binds only to a random `127.0.0.1` port and is terminated on cancellation or application shutdown.
- Whisper and Silero models are activated only after pinned size and SHA-256 verification.
- Provider responses must return every requested cue ID exactly once; missing, duplicate, reordered, or unknown IDs are rejected before persistence.
