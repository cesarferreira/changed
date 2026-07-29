# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

### Changed
- Filter filesystem events before re-querying git: ignored paths and irrelevant `.git` churn no longer trigger a refresh, and metadata-only events are skipped.
- Rate-limit git refreshes during sustained filesystem churn (750 ms minimum interval on top of the 120 ms debounce).
- Skip redraws when nothing visible changed; idle instances now wake up ~2x/s instead of redrawing 4x/s.
- Read the branch name from `git status --branch` instead of a separate `git rev-parse` subprocess per refresh.

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
[Unreleased]: https://github.com/cesarferreira/changed/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cesarferreira/changed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cesarferreira/changed/releases/tag/v0.1.0
