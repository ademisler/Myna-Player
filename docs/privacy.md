# Privacy and data handling

Myna Player is local-first. This document describes the current data flow so users and contributors can reason about privacy decisions.

## Local processing

The following operations run locally:

- media probing with FFprobe;
- audio-window extraction with FFmpeg;
- voice activity detection;
- speech recognition with whisper.cpp;
- subtitle timing, playback, editing, export, and cache management.

The application may store the canonical local media path, media fingerprint, playback position, transcript segments, provider-specific translations, processing checkpoints, settings, and model metadata in its local SQLite database.

## Optional cloud translation

Cloud translation is disabled by default. When a user selects and configures a cloud provider, Myna Player sends finalized transcript text and limited neighboring text context required for coherent translation. It does not send the video file or extracted audio to translation providers.

Provider retention and account policies are controlled by the selected provider. Users should review those policies before enabling a provider.

## Credentials

Provider credentials are stored through the operating system credential store. They must never be written to repository files, diagnostics, screenshots, or issue reports.

## Per-video deletion

The **Reset this video** action deletes that video's transcript, translations, processing checkpoints, cache record, and remembered playback position. The video remains open and returns to the beginning. Global settings, installed models, and credentials are not deleted.

## Full removal

Removing application data through the operating system deletes local history and generated subtitle data. Provider credentials may require separate removal from the operating system credential store or the application's provider settings.
