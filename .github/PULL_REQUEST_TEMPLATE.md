## What changed

Describe the user problem and the solution.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --all-features`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `trunk build --release`
- [ ] Relevant native/player/provider test completed

## Review checklist

- [ ] No credentials, private media, transcripts, model files, or generated build artefacts are included.
- [ ] Storage/schema/cache changes include migration and regression coverage.
- [ ] Translation changes preserve cue IDs and timing.
- [ ] UI changes are keyboard accessible and work at compact window sizes.
- [ ] Documentation and changelog are updated when appropriate.

## Screenshots or recordings

Add sanitized visuals for user-facing changes.
