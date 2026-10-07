# Myna Player engineering rules

- Keep media processing, scheduling, providers, storage, and UI in separate crates or modules.
- Never build shell command strings from user-controlled paths. Pass process arguments separately.
- Do not fake AI output. Missing providers must be represented as explicit unavailable states.
- Preserve original transcript and translated text as separate data.
- The player render loop may read cached cues only; it must never invoke ASR or translation directly.
- Every scheduling change requires deterministic unit tests, especially seek and end-of-file behavior.
- Prefer local processing by default. Any cloud provider must be visibly opt-in.
- Run formatting, targeted tests, native checks, and the Trunk build before committing.
