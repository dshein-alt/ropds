# ROPDS Docker deployment

Ready-to-use Docker Compose scenarios for running ROPDS with:

- SQLite (recommended default for most self-hosted setups)
- PostgreSQL 16+ (bundled or external)
- MariaDB 11+ (bundled or external)
- MySQL 8+ (external)

## Prerequisites

- Docker Engine + Docker Compose v2
- A host directory for your library (books, covers, uploads)

## Quick start (SQLite)

1. Create an environment file:

```bash
cp docker/.env.example docker/.env
```

2. Start the stack:

```bash
docker compose -f docker/docker-compose.sqlite.yml up -d --build
```

3. Open:

- Web UI: `http://localhost:8081/web`
- OPDS: `http://localhost:8081/opds`

## Compose scenarios

Each scenario has its own self-contained compose file — pick the one that matches your setup.

| Scenario | Command |
|---|---|
| SQLite (volume-backed DB) | `docker compose -f docker/docker-compose.sqlite.yml up -d --build` |
| PostgreSQL (bundled) | `docker compose -f docker/docker-compose.postgres.sibling.yml up -d --build` |
| PostgreSQL (external DB) | `docker compose -f docker/docker-compose.postgres.external.yml up -d --build` |
| MySQL/MariaDB (bundled) | `docker compose -f docker/docker-compose.mysql.sibling.yml up -d --build` |
| MySQL/MariaDB (external DB) | `docker compose -f docker/docker-compose.mysql.external.yml up -d --build` |

**Bundled** scenarios include both ROPDS and the database in a single compose file — one `docker compose up` starts everything.

**External** scenarios run ROPDS only and expect a database hosted elsewhere.

## Config files

Application configs live in `docker/config/`:

- `config.sqlite.toml` — Docker-ready default (SQLite), also used by the SQLite compose file
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

Edit the selected file as needed, especially:

- `server.base_url`: the externally reachable URL of your instance
- `[database].url`: for external DB scenarios

For local testing, `server.base_url = "http://localhost:8081"` is fine.

## Mounts and layout

Each compose file mounts:

- `./config/*.toml -> /app/config/config.toml` (read-only)
- `${ROPDS_LIBRARY_ROOT} -> /library` (read-write)

Web templates, static assets, and locales are baked into the release binary at build time.

At runtime the app creates and uses:

- `/library/covers`
- `/library/uploads`
- SQLite DB volume at `/var/lib/ropds/sqlite` (SQLite scenario only)

## Environment variables

See `docker/.env.example` for the full list. The most important ones:

| Variable | Default | Purpose |
|---|---|---|
| `TZ` | (none) | Container timezone (e.g. `Europe/Moscow`) |
| `ROPDS_PORT` | `8081` | Published HTTP port on the host |
| `ROPDS_LIBRARY_ROOT` | `../library` | Host path mounted as `/library` |
| `ROPDS_ADMIN_PASSWORD` | (none) | Admin bootstrap password |
| `ROPDS_ADMIN_INIT_ONCE` | `true` | One-time admin init via marker file |
| `ROPDS_DB_WAIT_TIMEOUT` | `60` | Seconds to wait for DB readiness |
| `ROPDS_DB_HOST` | (none) | Explicit DB host override |
| `ROPDS_DB_PORT` | (none) | Explicit DB port override |

For bundled DB scenarios (PostgreSQL and MySQL/MariaDB):

| Variable | Default | Purpose |
|---|---|---|
| `DB_NAME` | `ropds` | Database name |
| `DB_USER` | `ropds` | Database user |
| `DB_PASSWORD` | `ropds_change_me` | Database password |
| `DB_ROOT_PASSWORD` | `root_change_me` | MariaDB root password (MySQL only) |

These must match the credentials in the corresponding `config/*.toml`.

## Startup behavior

**Admin bootstrap.** The entrypoint can run `ropds --set-admin` automatically, controlled by `ROPDS_ADMIN_PASSWORD` in `docker/.env`. With `ROPDS_ADMIN_INIT_ONCE=true` (default) this happens once and drops a marker at `/library/.ropds_admin_initialized`. Set it to `false` to force a password reset on every restart.

**DB wait.** For PostgreSQL and MySQL URLs, the entrypoint waits for the database port to become reachable before starting the app. Override the wait target with `ROPDS_DB_HOST` and `ROPDS_DB_PORT`.

**Migrations.** They run automatically on startup, selected by the configured database backend.

## Security notes

- Change `ROPDS_ADMIN_PASSWORD` and `session_secret` before going to production.
- Use an absolute path for `ROPDS_LIBRARY_ROOT` in production.
- Keep the mounted config file read-only.
- Keep `session_secret` stable across restarts to preserve user sessions.
- `ROPDS_ADMIN_PASSWORD` is passed as a CLI argument during admin init and may appear in the process list (`/proc/PID/cmdline`, `docker inspect`). For sensitive environments, run `ropds --set-admin` manually inside the container instead.

## Library layout

- `covers_path` and `upload_path` default to subdirectories of `/library`.
- Cover settings are in `[covers]` (`covers_path`, `cover_max_dimension_px`, `cover_jpeg_quality`, `show_covers`).
- Keep only supported book formats in your library folders to avoid scanning non-book files.

## Reverse proxy

For production HTTPS behind Nginx or Traefik, see [`../service/proxy/README.md`](../service/proxy/README.md).
