# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
