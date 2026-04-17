# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.5] - 2026.04.17

### Added
- Official multi-architecture Docker images (linux/amd64, linux/arm64) are now published to GitHub Container Registry and Docker Hub on every release.
- Release pages now include ready-to-use compose files, config examples, and a sample `.env` so users can deploy ROPDS without cloning the repository. See [docker/README.md](docker/README.md) for the standalone deployment flow.

### Changed
- The `docker-compose.*.yml` files now default to pulling the published image instead of building from source; the library-mount default is now `./library` relative to the compose file.

## [0.10.4] - 2026.04.17

### Changed
- Documented supported database versions for SQLite, PostgreSQL, MySQL, and MariaDB in the main, Docker, and migration guides.

### Fixed
- Improved SQL compatibility for catalog and genre queries across PostgreSQL, MySQL, and MariaDB.

### Added
- Added regression coverage for cross-database query compatibility in PostgreSQL and MySQL/MariaDB test runs.

## [0.10.3] - 2026.04.17

This release closes [issue #4](https://github.com/dshein-alt/ropds/issues/4).

### Added
- Added a dedicated `--init-db` migration-preparation mode for PostgreSQL, MySQL, and MariaDB targets.

### Changed
- Reworked the SQLite-to-PostgreSQL/MySQL/MariaDB migration workflow to use a safer target preparation and import flow.
- Updated English and Russian migration documentation for the new database migration process.

### Fixed
- Redacted database credentials from startup log messages.
- Improved MariaDB URL handling across startup and container database wait helpers.
- Fixed PostgreSQL prefix-based grouping queries.

## [0.10.2] - 2026.04.06

### Changed
- Update project depemdencies.

### Fixed
- Docker build error due to missing build.rs script in the builder stage.

## [0.10.1] - 2026.03.05

### Changed
- Improved startup configuration checks so server base URL setup issues are detected early.
- Refined English and Russian documentation wording and structure for faster onboarding and clearer guidance.
- Updated Docker and deployment examples to better match current project configuration.

## [0.10.0] - 2026.03.04

### Added
- Added OAuth sign-in support for Google, Yandex, and Keycloak with controlled access approval.
- Added admin access-request moderation flow with approve, reject, ban, reinstate, and account-link options.
- Added OPDS password regeneration for OAuth users in the profile page.
- Added optional SMTP notifications for new and repeated OAuth access requests.

### Changed
- Updated project documentation with OAuth setup, approval workflow, and SMTP configuration guidance.

## [0.9.0] - 2026.03.03

### Added
- Added broader scanner regression coverage for archive deduplication and availability-state handling.

### Changed
- Refactored scanner internals into dedicated modules for book parsing, archive handling, database operations, INPX processing, and cover storage.
- Improved scan throughput and stability for large libraries with deferred batched writes and in-memory lookup optimization.
- Improved INPX processing flow with streaming group handling and safer worker coordination.

### Fixed
- Fixed scan availability transitions so deleted books are not reactivated during verification.
- Improved resilience when processing partially unreadable ZIP archives.
- Fixed scanner availability confirmation behavior to only update books in the expected intermediate state.

## [0.8.9] - 2026.03.03

### Added
- Added a SQLite-to-PostgreSQL/MySQL migration script with full data transfer, logging, and migration-state validation.
- Added support for running target DB commands through a container runtime when database client tools are not installed on the host.
- Added step-by-step migration guides for PostgreSQL and MariaDB in English and Russian.

### Changed
- Updated project documentation with direct references to database backend migration workflows.

### Fixed
- Fixed PostgreSQL reader progress handling so read history and last-read updates work correctly.

## [0.8.8] - 2026.03.03

### Changed
- Improved bookshelf removal behavior so related reading history entries are cleared when a book is removed from a bookshelf.

### Fixed
- Fixed MySQL compatibility for suppressed-book records and lookup behavior.

## [0.8.7] - 2026.03.02

### Added
- Added duplicate-version management in the web admin, including direct navigation to all versions and per-book delete actions.
- Added suppression for deleted archive-indexed books so they are not re-imported on future scans.
- Added per-user upload placement so newly published uploads are stored in user-specific directories.

### Changed
- Updated cover storage lookup to support current and previous directory layouts with automatic fallback behavior.
- Improved scanner resilience so parallel worker failures do not stop the whole scan.
- Expanded automated test coverage for duplicate management, suppression behavior, and upload destination rules.

### Fixed
- Fixed footer random-book rendering so missing covers show the `no cover` placeholder.
- Fixed MySQL suppression compatibility for long library paths.
- Fixed duplicate-versions search pagination so page limits and offsets apply consistently.

## [0.8.6] - 2026.03.02

### Changed
- Improved library scan parallelism with dynamic worker scheduling for more even processing of large collections.
- Improved INPX processing performance by parallelizing referenced archive enrichment while keeping global worker limits.
- Replaced legacy scanner parallelism dependency usage with the current task-based runtime approach.

### Fixed
- Improved scanner resource control so heavy scan and parsing work consistently respects shared concurrency limits.
- Fixed scanner integration checks to match hierarchical cover storage paths.

## [0.8.5] - 2026.03.02

### Changed
- Improved cover storage scalability for large libraries by introducing a hierarchical covers directory layout.
- Added automatic transition behavior so existing covers in the previous flat layout continue to work.

### Fixed
- Improved cover processing consistency so non-JPEG cover images, including GIF, are converted using configured cover settings.

## [0.8.4] - 2026.03.02

### Changed
- Improved author name normalization so multi-part names are preserved as entered.

### Fixed
- Fixed random book cover stretching in the footer.

## [0.8.3] - 2026.03.02

### Changed
- Improved INPX library import consistency so archive processing order is stable between scan runs.

### Fixed
- Fixed INPX imports to correctly retain book annotations and cover images from indexed archives.
- Improved INPX import resilience when referenced archives are missing.

## [0.8.2] - 2026.02.27

### Added
- Added support for newer OPDS 2.0 clients while keeping compatibility with existing OPDS clients.
- Added more OPDS browsing sections, including authors, series, genres, recent books, bookshelf, and language choices.

### Changed
- Improved internal OPDS organization to make future updates easier.

### Fixed
- Improved OPDS bookshelf privacy checks to better protect user-specific reading data.

## [0.8.1] - 2026.02.27

### Added
- Added Progressive Web App (PWA) support for the web interface, including installable behavior and service-worker-backed assets.
- Added OPDS "Recently Added" navigation and feed support.
- Added a web "Recent" page and continue-reading blocks on the home page.
- Added reader table of contents sidebar with localized labels.
- Added reading progress indicators on book lists and bookshelf cards.
- Added an admin duplicates page with localized labels.

### Changed
- Improved static content delivery with response compression and stronger cache handling.
- Updated theme behavior to follow system preference until the user selects an override.
- Expanded integration and docker test coverage for recent flows, PWA behavior, and duplicate detection.
- Reworked internal web module organization and shared context caching for cleaner maintenance.
- Updated documentation for reverse proxy setup, PWA usage, and duplicate management.

### Fixed
- Improved duplicate grouping consistency for books with multi-author metadata.
- Improved reliability of duplicate update operations across supported databases.

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
