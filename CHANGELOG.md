# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] - 2026.02.27

### Added
- Added self-contained web delivery by embedding UI assets and localization files into the release binary.
- Added release-profile static route checks in CI to validate production asset behavior.
- Added PWA manifest and service worker support for installable mobile web UI.
- Added reverse proxy documentation snippets for Nginx and Traefik.

### Changed
- Improved static content caching behavior to reduce stale client assets after upgrades.
- Simplified container packaging and runtime assumptions for web resources.

### Fixed
- Reduced startup overhead and improved response metadata handling for static content delivery.

## [0.7.5] - 2026.02.25

### Added
- Added OPDS language facet navigation with localized language choices.
- Added alphabet drill-down and pagination for OPDS author and series browsing.
- Added integration tests for OPDS language facet routes.

### Changed
- Expanded OPDS feed localization coverage for English and Russian.
- Updated language filtering so "all languages" browsing works consistently in OPDS feeds.
- Added and published a coverage report for unit and integration tests.

### Fixed
- Improved OPDS compatibility for cover thumbnail links in clients.
- Fixed OPDS OpenSearch behavior in feed responses.

## [0.7.4] - 2026.02.25

### Changed
- Enforced lockfile validation in Docker build stage.

### Fixed
- Fixed OPDS catalog, author, series, and book browsing returning 400 errors on base routes with axum 0.8 clients.

## [0.7.3] - 2026.02.25

### Added
- Added a systemd service unit template and short setup instructions for service deployment.

### Changed
- Unified cover image settings handling across library scanning, uploads, and OPDS cover generation.
- Made PDF and DjVu cover generation consistently follow configured cover size and JPEG quality.

### Fixed
- Applied reliability and maintenance cleanups across parsing and database query paths.

## [0.7.2] - 2026.02.24

### Added
- MOBI metadata and cover extraction during library scanning.
- Expanded automated test coverage for MOBI import workflows.

### Changed
- Updated project documentation and highlights for current format support.
- Added benchmark results documentation.

### Fixed
- Improved security of automated release notes publishing.

## [0.7.1] - 2026.02.24

### Added
- Decorative page frame borders and center divider for EPUB, FB2, and MOBI reader.
- Split logging output: debug/info/trace to stdout, warn/error to stderr.

### Changed
- Moved theme switcher, language selector, and account menu from the first navbar row into the search bar row; nav links now spread evenly across the first row.

### Fixed
- Reader footer toolbar now only renders for foliate-based formats (EPUB, FB2, MOBI).
- Fixed page frame border misalignment caused by unreliable dynamic content scanning; replaced with static CSS positioning.

## [0.7.0] - 2026.02.20

### Added
- Embedded book reader for EPUB, FB2, MOBI, DjVu, and PDF formats with reading position save/restore, reading history sidebar, and quick-access navbar button.
- Reader controls: page navigation (buttons, keyboard, mouse wheel, swipe), progress slider, location counter, go-to-page, font zoom, and background color presets — with mobile-responsive layout.
- `[reader]` configuration section with `enable` and `read_history_max` options.
- Cover image resize and compression on save with configurable dimensions.
- `[covers]` configuration section (extracted from `[library]` and `[opds]`) with admin settings panel.

### Changed
- Refactored covers configuration into a dedicated `[covers]` section.

## [0.6.2] - 2026.02.20

### Added
- Series editing for admin users on book upload and existing books.

### Changed
- Reorganized database migrations into per-backend subdirectories.

### Fixed
- Added version-based cache-busting for static assets to prevent stale browser cache after upgrades.

## [0.6.1] - 2026.02.20

### Added
- CI pipeline for running tests on commits and extended database tests on releases.
- CI pipeline for parsing changelog for release tags.

### Changed
- Consolidated Docker Compose files into standalone deployment scenarios.

## [0.6.0] - 2026.02.19

### Added
- Docker deployment bundle covering SQLite, sibling PostgreSQL/MySQL, and external DB connections.
- Russian Docker documentation.
- Docker integration tests for PostgreSQL and MySQL/MariaDB backends.
- Alphabet drill-down for OPDS books feed.
- Unified breadcrumb and back navigation across all web pages.
- Expanded unit and integration tests across all modules.

### Changed
- Refactored database pool into a cross-backend abstraction with automatic query rewriting.
- Replaced raw integer constants with typed enums for catalog types and availability statuses.
- Squashed development migrations into clean initial state per backend.
- Extracted library crate from binary for integration test support.
- Backend-specific migrations are now embedded and selected at runtime.
- Bumped minor dependency versions.
- Tightened Docker runtime defaults (healthcheck, admin bootstrap, DB wait, container paths).

### Fixed
- Fixed PostgreSQL backend support.
- Fixed MySQL/MariaDB backend support.
- Fixed EPUB parsing when multiple rootfiles are present.

## [0.5.0] - Initial Release

### Added
- Initial public release of ROPDS.
- OPDS catalog and web interface.
- Library scanning and metadata extraction.
- SQLite, PostgreSQL, and MySQL/MariaDB backend support.
- User authentication, admin controls, and upload features.
