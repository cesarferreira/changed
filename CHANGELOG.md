# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

### Fixed
- Show unmerged (conflict) files during rebase and merge — `git status --porcelain=v2` reports these as `u` lines, not `1`.

## [0.4.0] - 2026-07-30

### Changed
- Fix stale UI in linked worktrees by watching external git dir.

## [0.3.0] - 2026-07-29

### Changed
- Avoid contending for .git/index.lock during background polling
- Enhancements
- Better gradient
- Filter watcher events and rate-limit git refreshes (#1)

## [0.2.0] - 2026-07-28

### Changed
- Mvp
- Fade green background on changed files instead of underline flash.
- Rename crate to changed-cli and fix rustfmt CI failures.
- Show clean state once and smooth the green flash fade.
- Alignment
- Fix release

## [0.1.0] - 2026-01-01

### Added
- Initial release

<!-- next-url -->
[Unreleased]: https://github.com/cesarferreira/changed/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/cesarferreira/changed/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/cesarferreira/changed/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/cesarferreira/changed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cesarferreira/changed/releases/tag/v0.1.0
