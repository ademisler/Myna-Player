# Architecture

## Product boundary

SubAhead is a media player with a just-in-time subtitle pipeline. It is not a generic transcription editor. For local files, analysis reads directly from the media file so processing can stay ahead of playback instead of waiting for audible system output.

## Workspace

- `subahead-ui`: Leptos CSR interface compiled to WebAssembly.
- `src-tauri`: native application shell and narrow command boundary.
- `subahead-core`: serializable domain models and deterministic scheduling.
- `subahead-media`: FFprobe inspection and FFmpeg audio extraction.
- `subahead-pipeline`: ASR and translation provider contracts.

## Processing contract

1. Read playback position from the player.
2. Compare it with `ready_until_ms`.
3. Schedule 30-second windows until the 90-second target buffer is filled.
4. Extract each window as mono, 16 kHz, signed 16-bit PCM.
5. Send speech regions to an ASR adapter.
6. preserve source text, timecodes, detected language, and confidence.
7. Translate finalized segments in context-aware batches.
8. Store source and translated cues separately.
9. Render cues based only on player time; never call AI from the render loop.

## Safety and privacy

- Local file paths stay in the native backend.
- Media processing commands use argument arrays rather than shell interpolation.
- Cloud translation must be opt-in and clearly label which text leaves the device.
- API keys will be stored through the operating system credential store, not plaintext configuration.
