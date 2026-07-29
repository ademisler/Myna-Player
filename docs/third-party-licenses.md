# Third-party runtime notices

Myna Player is licensed under GPL-3.0-or-later. Its macOS distribution stages the
following runtime without copying source code or artwork from the referenced
player applications:

- VLC / libVLC 3.0.21, Copyright VideoLAN and VLC authors. Distributed under
  GPL-2.0-or-later and the component-specific compatible licenses documented by
  VideoLAN. The packaging script includes VLC's `COPYING` file in
  `Contents/Resources/licenses/VLC-COPYING.txt`.
- FFmpeg and FFprobe are invoked as external local tools during development.
  Distribution packaging must include the matching license and configuration
  notice if these binaries are bundled in a release.
- whisper.cpp is invoked through a loopback-only long-lived worker. Distribution
  packaging must include its MIT license when the worker is bundled.

The IINA and Celluloid projects informed interaction research only. Their source
code and visual assets are not included in Myna Player.
