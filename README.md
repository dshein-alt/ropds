# ROPDS

[![CI](https://github.com/dshein-alt/ropds/actions/workflows/tag-tests.yml/badge.svg?branch=master)](https://github.com/dshein-alt/ropds/actions/workflows/tag-tests.yml)

**Rust OPDS Server** — a fast, lightweight personal e-book library server with OPDS catalog and web interface.

> Inspired by [SimpleOPDS](https://github.com/mitshel/sopds).
> Built from scratch in Rust as a modern, self-hosted alternative.

[Русская версия](README_RU.md)

## Why ROPDS?

The goal is simple: **a personal library server you can set up once and forget about.** Easy to deploy on a home server or VPS for yourself, family, and friends.

- **Fast & resource-friendly** — single Rust binary, no runtime dependencies, containers optional
- **Easy deployment** — SQLite out of the box with Docker bundles for PostgreSQL and MySQL/MariaDB
- **OPDS 1.2 compatible** — works with any OPDS reader (KOReader, Moon+ Reader, Librera, etc.)
- **Built-in book reader** — read EPUB, FB2, MOBI, DjVu, and PDF directly in the browser with position sync
- **Bookshelf & uploads** — personal reading lists and book uploads with auto-extracted metadata
- **Responsive web UI** — browse, search, and manage your library with light/dark theme

This project is also an **educational pet-project** exploring modern Rust ecosystem and heavy use of **competitive LLM coding agents** throughout development.

## Features

**Library Management**
- Automatic background scanning with configurable schedule
- Parallel scanning with Rayon thread pools
- Supports books inside ZIP archives and INPX index files
- Metadata extraction for FB2 and EPUB (title, authors, genres, series, covers, annotations)
- Optional PDF/DjVu cover generation via external tools (`pdftoppm`, `ddjvu`)

**OPDS Catalog**
- Full OPDS 1.2 Atom feeds with pagination
- Browse by authors, series, genres, catalogs, or title prefix
- OpenSearch support
- Cover thumbnails and full-size images
- HTTP Basic Auth (can be disabled)

**Search**
- Full-text search across book titles, authors, and series — from both OPDS and web UI
- Alphabetical prefix browsing with configurable split threshold for large collections
- OpenSearch descriptor for OPDS client integration

**Bookshelf**
- Personal reading list per user with one-click add/remove
- Books are automatically added to the bookshelf on download
- Sort by date added, title, or author — ascending or descending
- Infinite scroll for seamless browsing

**Book Upload**
- Upload books directly through the web interface (FB2, EPUB, PDF, and other supported formats)
- Metadata is auto-extracted on upload with immediate editing — adjust title, authors, and genres before saving
- Per-user upload permissions controlled by the admin

**Genres**
- Hierarchical genre system with sections and subcategories
- Per-language genre translations stored in the database
- Admin UI for creating sections, adding genres, and managing translations
- Flexible tagging — assign multiple genres per book, edit anytime

**User Management**
- Multi-user support with built-in admin panel
- Create and delete users, reset passwords, toggle upload permissions
- Users manage their own profile: display name and password
- Forced password change on first login when set by admin

**Embedded Book Reader**
- Read EPUB, FB2, MOBI, DjVu, and PDF directly in the browser — no downloads needed
- Automatic reading position save/restore per user per book
- Reading history sidebar with quick access to recently read books
- Reader opens in a new tab with an immersive full-page layout
- Powered by [foliate-js](https://github.com/johnfactotum/foliate-js) and [djvu.js](https://github.com/RussCoder/djvujs)

**Web Interface**
- Responsive Bootstrap 5 UI with light/dark theme
- Browse by catalogs, authors, series, or genres with breadcrumb navigation
- Inline book metadata editing for admins (title, authors, genres)
- Cover preview with full-size overlay on click

**Internationalization**
- Ships with **English** and **Russian** locales out of the box
- Locale files are simple TOML — adding a new language is as easy as copying `en.toml` and translating the strings
- Genre names support per-language translations stored in the database
- Per-user language preference saved in cookie

**Security**
- Argon2 password hashing
- HMAC-SHA256 signed session cookies
- Configurable session lifetime
- Per-user upload permissions
- Superuser role for admin access

## Quick Start

### 1. Build

```bash
cargo build --release
```

### 2. Configure

```bash
cp config.toml.example config.toml
```

Edit `config.toml` — at minimum set `library.root_path` to your book collection:

```toml
[library]
root_path = "/path/to/books"

[covers]
covers_path = "/path/to/books/covers"
cover_max_dimension_px = 600
cover_jpeg_quality = 85
show_covers = true

[database]
url = "sqlite://ropds.db?mode=rwc"
```

### 3. Create admin user

```bash
./target/release/ropds --set-admin <password>
```

### 4. Run

```bash
./target/release/ropds
```

The server starts at `http://localhost:8081`. Open `/web` for the web UI or point your OPDS reader at `/opds`.

### One-shot scan

To scan the library once without starting the server:

```bash
./target/release/ropds --scan
```

## Docker

For containerized deployment, use the ready-to-run bundle in [`docker/`](docker/):

- English guide: [`docker/README.md`](docker/README.md)
- Russian guide: [`docker/README_RU.md`](docker/README_RU.md)

## Configuration

All settings live in `config.toml`. See [config.toml.example](config.toml.example) for a fully commented reference.

| Section | Key highlights |
|---|---|
| `[server]` | Bind address, port, log level, session secret & TTL |
| `[library]` | Book root path, file extensions, ZIP/INPX support |
| `[covers]` | `covers_path`, resize/compression (`cover_max_dimension_px`, `cover_jpeg_quality`), `show_covers` |
| `[database]` | Connection URL — `sqlite://`, `postgres://`, or `mysql://` |
| `[opds]` | Catalog title, pagination, auth |
| `[scanner]` | Cron-style schedule, parallel workers, integrity checks |
| `[web]` | Default language (`en`, `ru`), default theme (`light`, `dark`) |
| `[upload]` | Enable/disable uploads, staging directory, size limit |
| `[reader]` | Enable/disable embedded reader, reading history size |

## Supported Formats

| Format | Metadata | Covers |
|---|---|---|
| FB2 | Full (title, authors, genres, series, annotation, language) | Embedded |
| EPUB | Full (OPF metadata) | Embedded |
| PDF | Title, author (via `pdfinfo`) | First page (via `pdftoppm`) |
| DjVu | Filename only | First page (via `ddjvu`) |
| MOBI | Filename only | — |
| DOC, DOCX | Filename only | — |

Books inside **ZIP archives** are scanned transparently. **INPX** index files are supported as an alternative to scanning individual archives.

## Database

SQLite is the default and recommended choice — no setup needed. PostgreSQL and MySQL are also supported via the `[database].url` setting.

Migrations run automatically on startup. Backend-specific migration sets are embedded at build time and selected by database backend (`sqlite://`, `postgres://`, `mysql://`).

## Tech Stack

| | |
|---|---|
| Language | Rust (edition 2024) |
| Web framework | Axum 0.8 |
| Async runtime | Tokio |
| Database | SQLx (SQLite / PostgreSQL / MySQL) |
| Templates | Tera |
| UI framework | Bootstrap 5 + Bootstrap Icons |
| Password hashing | Argon2 |
| XML parsing | quick-xml |
| Parallelism | Rayon + DashMap |

## Performance

See [BENCHMARK.md](BENCHMARK.md) for Apache Bench results (~29K req/s, ~135K req/s with keep-alive).

## License

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE), at your option.
