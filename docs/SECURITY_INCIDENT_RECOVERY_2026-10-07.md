# Myna Player Security Incident Recovery — 2026-10-07

## Summary

The original `ademisler/Myna-Player` repository was treated as compromised after malicious payloads were found in Dependabot pull-request refs/history. The trusted `main` source tree itself was independently reviewed and remained clean.

The compromised repository was renamed to `ademisler/Myna-Player-compromised-archive-20261007`, made private, and archived read-only. A new empty, non-template public repository was then created as `ademisler/Myna-Player` and populated from a fresh one-root Git repository reconstructed only from the reviewed clean source tree.

## Repository identities

- Archived original repository ID: `1314307883`
- Archived original repository: `ademisler/Myna-Player-compromised-archive-20261007`
- Rebuilt repository ID: `1409176639`
- Rebuilt repository: `ademisler/Myna-Player`

## Trusted source

- Trusted source commit: `bf1c23a73977fe250dd6a44473a3593adc16bfba`
- Trusted source tree: `ec43c0cfb0fa74edbc4b4e3757153e0cd23c8f55`
- Tracked files: `149`
- Clean root commit: `5eb4806b8532a24e026e0d1d96b5f7976d84faae`
- Clean root tree: `ec43c0cfb0fa74edbc4b4e3757153e0cd23c8f55`

The fresh Git index reproduced the trusted Git tree exactly before the root commit was created.

## Known malicious indicators

Representative malicious commits from the compromised repository:

- `efc8219d13984d92008057bccb8b48705a36c7f5`
- `e816333aa97f08059086303a0f0959f304ebfc43`
- `f116a2b0d97a4fcd5dc73ce8e633e68d78cac140`
- `bf0af716d9076a0de35de0de0cc3561afe67d725`

Representative malicious blob/object indicators:

- `50d61c1bbbcd89f4e6d9c9a34f8d3bf2fdacd3ea`
- `27338d3d89f4e6d9c9a34f8d3bf2fdacd3ea`

Observed malware behavior included VS Code folder-open autorun configuration and JavaScript disguised as a font payload.

All representative bad commit/blob SHAs above returned HTTP 404 when queried against the rebuilt repository after cutover.

## Validation

- Reviewed clean source tree reproduced exactly: PASS
- Fresh tracked-file count: 149
- `git fsck --full --strict`: PASS
- Fresh reachable history at root cutover: one root commit
- Old bad commit/object resolution in rebuilt repo: PASS (not found)
- Archived original: private + archived
- New repository: public + non-template
- Active malicious Dependabot PRs were closed before cutover
- Active malicious Dependabot branches were removed before cutover

No dependency install, project build, or executable repository code was run during source reconstruction. Full isolated build/test remains pending because the available Modal environment could be leased but remote source execution was blocked by the control-plane safety boundary. This is a test-coverage limitation, not a source-integrity failure.

## Post-cutover isolated CI verification

GitHub-hosted runners executed the rebuilt repository after cutover, so the earlier Modal execution limitation is no longer the only available runtime evidence.

- Quality run `37663328116` on rebuilt main:
  - formatting: PASS
  - workspace tests with every feature: PASS
  - Windows native compile: PASS
  - advisories/licenses/sources (`cargo deny`): PASS
  - Clippy: FAIL only on two Rust 1.99 `clippy::double_must_use` diagnostics generated through `async_trait` in `crates/myna-player-pipeline/src/lib.rs` (traits `AsrEngine` and `TranslationProvider`)
  - Leptos release build: skipped because the Clippy step failed first
- Native package smoke run `37663328023`:
  - clean macOS arm64 standalone bundle: PASS
  - bundled libVLC playback/replay exercise: PASS
  - Windows x64 package: FAIL in `scripts/build-ffmpeg-sidecars-windows.ps1` because MSYS2 `tar` interpreted the runner's `D:` path as a remote host (`tar: Cannot connect to D: resolve failed`), after libVLC staging and the pinned whisper sidecar build had already passed

These two remaining CI failures are ordinary post-recovery code/tooling issues and do not indicate repository contamination or a source-integrity failure.

Fresh Dependabot branches created after cutover were separately reviewed. Their commits are GitHub-verified, contain no known incident IOC paths, and change only `Cargo.lock` (Rust dependency update) or the three pinned workflow files (GitHub Actions update).

## Metadata and credentials

Restored repository metadata includes the original description, homepage, topics, and merge settings.

The archived repository exposed no repository-level Actions secret names, Actions variables, environments, deploy keys, or webhooks at recovery time. No old secret values or deploy credentials were copied to the rebuilt repository.

## Persistent-host review

Ternrise was searched for Myna Player checkout/worktree paths and none were found. No production or persistent Myna Player checkout required Git database replacement or runtime cleanup.

No deployment hold was required because no active Myna Player deployment trust path was identified on Ternrise during recovery.

## Backup artifacts

- Clean source tarball SHA-256: `065cc276365e20c6c0ee879d7307a4ad8a4b6db0b6cbc82c897ce2f7c4675daa`
- Tracked-file SHA-256 manifest SHA-256: `453298407c75a30650576fe1ebd1b379e6a741aef1841674271fad12a9a0ea17`
- Fresh root Git bundle SHA-256: `ee45dbb35161959a7587dde8bb39b88da19efe763e62f99c225e40b0790e350e`

## Recovery rule

Do not reintroduce Git object databases, refs, worktrees, caches, editor autorun configuration, or credentials from the archived compromised repository into the rebuilt repository. Future dependency-update branches must be treated as untrusted until the account/session compromise path is fully understood.
