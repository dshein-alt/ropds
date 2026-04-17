# ROPDS

[![CI](https://github.com/dshein-alt/ropds/actions/workflows/tag-tests.yml/badge.svg?branch=master)](https://github.com/dshein-alt/ropds/actions/workflows/tag-tests.yml)

Fast, lightweight self-hosted e-book library server with an OPDS 1.2 / 2.0 catalog and a web UI. Built in Rust.

> Inspired by [SimpleOPDS](https://github.com/mitshel/sopds), rebuilt from scratch as a modern alternative for home servers and small VPS instances.

[Русская версия](README_RU.md)

## What you get

- Single Rust binary, no runtime dependencies, containers optional
- OPDS 1.2 / 2.0 feeds compatible with popular readers (CoolReader, FBReader, Librera, KOReader, etc.)
- Web UI with browsing, search, admin panel, book uploads, and a built-in reader
- Multi-user accounts with per-user upload permissions
- OAuth sign-in (Google, Yandex, Keycloak OIDC) with approval queue and optional email notifications
- Background library scanning including ZIP archives and INPX index files
- Light and dark themes, installable as a PWA

The goal is a personal library server you set up once and forget about — easy to deploy for yourself, family, and friends.

This project doubles as an **educational pet project** exploring the modern Rust ecosystem, with heavy use of **competitive LLM coding agents** throughout development.

## Quick start

### 1. Build

```bash
cargo build --release
```

### 2. Configure

```bash
cp config.toml.example config.toml
```

Minimum required settings:

```toml
[server]
base_url = "http://localhost:8081"

[library]
root_path = "/path/to/books"
```

Full example with covers and database:

```toml
[server]
base_url = "http://localhost:8081"

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

Open:

- Web UI: `http://localhost:8081/web`
- OPDS: `http://localhost:8081/opds`

### One-shot scan

Scan the library once without starting the server:

```bash
./target/release/ropds --scan
```

## Features

### Library management

- Background scanning on a configurable cron schedule
- Parallel scanning with worker-limited dynamic task scheduling
- Books inside ZIP archives and INPX index files are handled transparently
- Metadata extraction for FB2, EPUB, and MOBI — title, authors, genres, series, covers, annotations
- Optional cover generation for PDF and DjVu via external tools (`pdftoppm`, `ddjvu`)

### OPDS catalog

- Full OPDS 1.2 / 2.0 feeds with pagination
- Browse by author, series, genre, catalog, or title prefix
- OpenSearch support
- Cover thumbnails and full-size images
- HTTP Basic Auth (can be disabled)

### Search

- Full-text search across titles, authors, and series — from both OPDS and the web UI
- Alphabetical prefix browsing with configurable split threshold for large collections
- OpenSearch descriptor for OPDS client integration

### Bookshelf

- Personal reading list per user — add or remove books with one click
- Books are automatically added to the bookshelf on download
- Sort by date added, title, or author in either direction
- Infinite scroll

### Book upload

- Upload books directly through the web interface (FB2, EPUB, PDF, and other supported formats)
- Metadata is extracted automatically with immediate editing — adjust title, authors, and genres before saving
- Per-user upload permissions controlled by the admin

### Genres

- Hierarchical genre system with sections and subcategories
- Per-language genre translations stored in the database
- Admin UI for creating sections, adding genres, and managing translations
- Flexible tagging — multiple genres per book, editable at any time

### User management

- Multi-user support with a built-in admin panel
- Create and delete users, reset passwords, toggle upload permissions
- Users manage their own profile: display name and password
- OAuth users can regenerate a dedicated OPDS password from their profile
- Forced password change on first login when set by admin

### OAuth and access requests

- Providers: Google, Yandex, Keycloak (OIDC)
- New OAuth users enter a pending state until an administrator approves them
- Admins can approve, reject, ban, or reinstate access requests
- Approval supports linking an OAuth identity to an existing local account
- Keycloak: optional auto-approval and role-based mapping for upload and admin permissions
- Optional SMTP notifications to admin on new and re-applied requests

### Embedded book reader

- Read EPUB, FB2, MOBI, DjVu, and PDF directly in the browser — no downloads required
- Automatic reading position save and restore per user per book
- Reading history sidebar with quick access to recently read books
- Opens in a new tab in browsers and in the same window when installed as a PWA
- Powered by [foliate-js](https://github.com/johnfactotum/foliate-js) and [djvu.js](https://github.com/RussCoder/djvujs)

### Web interface

- Responsive Bootstrap 5 UI with light and dark themes
- Installable as a PWA on mobile and desktop (manifest + service worker)
- Browse by catalog, author, series, or genre with breadcrumb navigation
- Inline book metadata editing for admins (title, authors, genres)
- Duplicates page: duplicate editions grouped by title + authors, with pagination
- Cover preview with full-size overlay on click

### Internationalization

- Ships with **English** and **Russian** locales
- Locale files are plain TOML — adding a language is as easy as copying `en.toml` and translating the strings
- Genre names support per-language translations in the database
- Per-user language preference saved in a cookie

### Security

- Argon2 password hashing
- HMAC-SHA256 signed session cookies
- Configurable session lifetime
- Per-user upload permissions
- Superuser role for admin access

## Configuration

All settings live in `config.toml`. See [config.toml.example](config.toml.example) for a fully commented reference.

`server.base_url` is required — it is used for OAuth callback URLs and links in admin notification emails.

| Section | Key highlights |
|---|---|
| `[server]` | Bind address, port, log level, session secret, TTL, `base_url` |
| `[library]` | Book root path, file extensions, ZIP/INPX support |
| `[covers]` | `covers_path`, resize and compression (`cover_max_dimension_px`, `cover_jpeg_quality`), `show_covers` |
| `[database]` | Connection URL — `sqlite://`, `postgres://`, or `mysql://` |
| `[opds]` | Catalog title, pagination, auth |
| `[scanner]` | Cron schedule, parallel workers, integrity checks |
| `[web]` | Default language (`en`, `ru`), default theme (`light`, `dark`) |
| `[upload]` | Enable/disable uploads, staging directory, size limit |
| `[reader]` | Enable/disable embedded reader, reading history size |
| `[oauth]` | Provider credentials, moderation settings, Keycloak role mapping, notification toggle |
| `[smtp]` | SMTP server settings for outbound email notifications |

## OAuth login and approval

1. Set `server.base_url` to your externally reachable URL.
2. Configure at least one provider in `[oauth]` (`google_*`, `yandex_*`, or Keycloak settings).
3. (Optional) Enable admin notifications: set `oauth.notify_admin_email = true` and fill in `[smtp]`.
4. Users sign in via `/web/login`.
5. New users land in **Admin -> Access Requests** until approved.

Minimal example (Google + admin email notifications):

```toml
[server]
base_url = "https://books.example.com"

[oauth]
google_client_id = "..."
google_client_secret = "..."
notify_admin_email = true

[smtp]
host = "smtp.example.com"
port = 587
username = "smtp-user"
password = "smtp-pass"
from = "ropds@example.com"
send_to = ["admin@example.com", "alerts@example.com"]
starttls = true
```

## Deployment

### Systemd

Use the template unit file from `service/ropds.unit` (runs under the `ropds` user account).

```bash
sudo useradd --system --home /opt/ropds --shell /usr/sbin/nologin ropds || true
sudo install -d -o ropds -g ropds /opt/ropds
sudo install -m 0755 target/release/ropds /opt/ropds/ropds
sudo install -m 0644 config.toml /opt/ropds/config.toml
sudo install -m 0644 service/ropds.unit /etc/systemd/system/ropds.service
sudo systemctl daemon-reload
sudo systemctl enable --now ropds.service
sudo systemctl status ropds.service
sudo journalctl -u ropds.service -f
```

### Docker

Ready-to-run bundle with compose files for SQLite, PostgreSQL, and MySQL/MariaDB:

- English guide: [`docker/README.md`](docker/README.md)
- Russian guide: [`docker/README_RU.md`](docker/README_RU.md)

### Reverse proxy

Nginx and Traefik snippets:

- English: [`service/proxy/README.md`](service/proxy/README.md)
- Russian: [`service/proxy/README_RU.md`](service/proxy/README_RU.md)

## Supported formats

| Format | Metadata | Covers |
|---|---|---|
| FB2 | Full (title, authors, genres, series, annotation, language) | Embedded |
| EPUB | Full (OPF metadata) | Embedded |
| MOBI | Full (title, author, description, language, date) | Embedded |
| PDF | Limited (title, author via `pdfinfo`) | First page (via `pdftoppm`) |
| DjVu | Filename only | First page (via `ddjvu`) |
| DOC, DOCX | Filename only | — |

Books inside **ZIP archives** are scanned transparently. **INPX** index files are supported as an alternative to scanning individual archives.

## Database

SQLite is the default and simplest option — no setup needed. PostgreSQL and MySQL are also supported via `[database].url`.

For SQLite, a scanner parallelism setting of `workers_num = 2..4` is usually the sweet spot. Higher values can increase write-lock contention during large rescans.

Migrations run automatically on startup. Backend-specific migration sets are embedded at build time and selected by the database URL prefix (`sqlite://`, `postgres://`, `mysql://`).

### Migrating between backends (SQLite -> PostgreSQL or MySQL/MariaDB)

Four-step flow:

1. **Create the role and database** on the target (one-time). The role only needs to own the DB — no superuser or `root` required.
2. **Prepare the target schema with `ropds --init-db`** — creates the database if missing, applies every migration, clears every user table so the target is truly empty, and exits. Refuses if the target already has rows (so it is safe to invoke accidentally against a live or already-migrated DB — you'll be told to reset it manually).
3. **Copy the data with `scripts/migrate_sqlite.py`** — minimal helper that only does truncate + copy + verify in a single CLI session. Precheck: every target data table must have 0 rows (the state `--init-db` leaves behind); otherwise the script lists the offenders and refuses. Requires interactive confirmation; depends only on `psql` or `mysql`/`mariadb`. Supports running those clients inside a container via `--db-container NAME --container-runtime {docker,podman}`.
4. **Start ROPDS** against the new URL.

- English step-by-step guide (with SQL examples for PG and MySQL/MariaDB): [`scripts/README.md`](scripts/README.md)
- Russian step-by-step guide: [`scripts/README_RU.md`](scripts/README_RU.md)

## Tech stack

| Component | Choice |
|---|---|
| Language | Rust (edition 2024) |
| Web framework | Axum 0.8 |
| Async runtime | Tokio |
| Database | SQLx (SQLite / PostgreSQL / MySQL) |
| Templates | Tera |
| UI | Bootstrap 5 + Bootstrap Icons |
| Password hashing | Argon2 |
| XML parsing | quick-xml |
| Parallelism | Tokio task queue + DashMap |

## Performance

Apache Bench results: [BENCHMARK.md](BENCHMARK.md) (~29K req/s, ~135K req/s with keep-alive).

## License

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE), at your option.
