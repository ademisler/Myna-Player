# Security policy

Myna Player handles local media paths, generated transcripts, translation-provider credentials, downloaded AI models, and native runtime binaries. Security and privacy reports are taken seriously.

## Supported versions

Myna Player is currently pre-1.0. Security fixes are applied to the latest `main` branch and the newest published release when releases are available. Older development snapshots are not supported.

## Reporting a vulnerability

Please **do not open a public issue** for a vulnerability.

Use GitHub's private vulnerability reporting flow:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Include affected versions or commits, reproduction steps, impact, and any suggested mitigation.

Reports involving exposed credentials should identify the provider but must not include active secret values in screenshots, logs, or issue text.

## Response goals

- Initial acknowledgement: within 5 business days.
- Triage and severity assessment: as soon as reproducibility permits.
- Coordinated disclosure: after a fix or mitigation is available.

These are goals rather than a service-level agreement for this volunteer-led project.

## Security boundaries

Expected local data includes application settings, playback positions, transcript segments, translations, processing checkpoints, and model files. Cloud translation is opt-in. The video and audio are not sent to translation providers; only finalized text is sent when a cloud provider is selected.

See [docs/privacy.md](docs/privacy.md) and [docs/security-exceptions.md](docs/security-exceptions.md).
