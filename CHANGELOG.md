# Changelog

All notable changes to dimtune are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-10-20

### Added

- Screenshot of the application in the README.
- This changelog.

### Changed

- Split the monolithic `src/main.rs` into focused modules: `app`, `config`,
  `library`, `metadata`, `style`, `ui`, and `util`.
- CI now runs only for source code, tests, build configuration, and workflow file
  changes. Documentation-only changes no longer trigger a build.
- Bumped the crate version to 0.2.1.

### Fixed

- Track loading and seeking no longer run on the UI thread. A slow file or a
  stalled audio device can no longer freeze the window or trigger a transient
  “not responding” dialog.
- Playback controls and progress repaint immediately after playback starts or
  pauses.

## [0.2.0] - 2026-10-19

### Changed

- Translated the entire application to English: UI labels, status messages,
  code comments, identifiers, scripts, and documentation.
- Renamed the install and uninstall scripts to `install.sh` and `uninstall.sh`.
- Renamed the configuration keys to `folder`, `shuffle`, and `volume`. This is a
  breaking change for existing config files; update the keys or delete the file
  so dimtune recreates it.
- Redesigned the README header with a centered icon, tagline, and project badges.
- Bumped the crate version to 0.2.0.

## [0.1.0] - 2026-10-19

### Added

- Initial public release.
- Native dark UI built with `eframe`/`egui`.
- Recursive folder scanning for MP3, FLAC, Ogg Vorbis, WAV, and M4A/AAC.
- Playback via `rodio`: play/pause, previous/next, seek, volume, and shuffle.
- Metadata and cover art via `lofty` and `image`.
- Native folder picker through the XDG Desktop Portal (`rfd`).
- Keyboard shortcuts.
- Per-user install/uninstall scripts, desktop entry, and icon.
- GitHub Actions CI and tagged release workflows.

[Unreleased]: https://github.com/Abemilek/dimtune/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Abemilek/dimtune/releases/tag/v0.2.1
[0.2.0]: https://github.com/Abemilek/dimtune/releases/tag/v0.2.0
[0.1.0]: https://github.com/Abemilek/dimtune/releases/tag/v0.1.0
