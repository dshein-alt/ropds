# ROPDS Docker Deployment

This folder contains a complete Docker deployment bundle for ROPDS with standalone compose files for each deployment scenario.

## Prerequisites

- Docker Engine + Docker Compose v2
- A library directory on host (books, covers, uploads)

## Quick start (SQLite, recommended default)

1. Create environment file:

```bash
cp docker/.env.example docker/.env
```

2. Start ROPDS with SQLite on a dedicated Docker volume:

```bash
docker compose -f docker/docker-compose.sqlite.yml up -d --build
```

3. Open:

- Web UI: `http://localhost:8081/web`
- OPDS: `http://localhost:8081/opds`

## Compose file matrix

Each scenario is a single self-contained compose file.

| Scenario | Command |
|---|---|
| SQLite (volume-backed DB) | `docker compose -f docker/docker-compose.sqlite.yml up -d --build` |
| PostgreSQL (bundled) | `docker compose -f docker/docker-compose.postgres.sibling.yml up -d --build` |
| PostgreSQL (external DB) | `docker compose -f docker/docker-compose.postgres.external.yml up -d --build` |
| MySQL/MariaDB (bundled) | `docker compose -f docker/docker-compose.mysql.sibling.yml up -d --build` |
| MySQL/MariaDB (external DB) | `docker compose -f docker/docker-compose.mysql.external.yml up -d --build` |

**Bundled** scenarios include both ROPDS and the database service in the same compose file — a single `docker compose up` starts everything.

**External** scenarios run ROPDS only and connect to a database hosted elsewhere.

## Config files

All app configs are in `docker/config/`:

- `config.toml.example`: valid Docker-ready default (SQLite), also used by the SQLite compose file
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

You can customize credentials/hosts directly in those files.

## Mount model

Each compose file mounts:

- `./config/*.toml -> /app/config/config.toml` (read-only)
- `${ROPDS_LIBRARY_ROOT} -> /library` (read-write)

Web templates, static assets, and locales are embedded into the release binary at build time.

Runtime creates/uses:

- `/library/covers`
- `/library/uploads`
- SQLite DB volume at `/var/lib/ropds/sqlite` (SQLite scenario)

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `TZ` | (none) | Container timezone (e.g. `Europe/Moscow`) |
| `ROPDS_PORT` | `8081` | Published HTTP port on host |
| `ROPDS_LIBRARY_ROOT` | `../library` | Host path for `/library` volume |
| `ROPDS_ADMIN_PASSWORD` | (none) | Admin bootstrap password |
| `ROPDS_ADMIN_INIT_ONCE` | `true` | One-time admin init via marker file |
| `ROPDS_DB_WAIT_TIMEOUT` | `60` | Seconds to wait for DB readiness |
| `ROPDS_DB_HOST` | (none) | Optional explicit DB host override |
| `ROPDS_DB_PORT` | (none) | Optional explicit DB port override |

For bundled DB scenarios (PostgreSQL and MySQL/MariaDB):

| Variable | Default | Purpose |
|---|---|---|
| `DB_NAME` | `ropds` | Database name |
| `DB_USER` | `ropds` | Database user |
| `DB_PASSWORD` | `ropds_change_me` | Database password |
| `DB_ROOT_PASSWORD` | `root_change_me` | MariaDB root password (MySQL only) |

These must match the credentials in the corresponding `config/*.toml` file.

## Admin bootstrap

Entrypoint supports admin initialization:

- `ROPDS_ADMIN_PASSWORD` must be set in `docker/.env`
- `ROPDS_ADMIN_INIT_ONCE=true` (default):
  - calls `ropds --set-admin ...` once
  - creates `/library/.ropds_admin_initialized`

To force password set each restart, set `ROPDS_ADMIN_INIT_ONCE=false`.

## DB wait + migrations

For PostgreSQL/MySQL URLs, entrypoint waits for DB port before starting app.
You can override wait target with `ROPDS_DB_HOST` and `ROPDS_DB_PORT`.

Migrations are executed by ROPDS on startup based on configured DB backend.

## Security notes

- Change `ROPDS_ADMIN_PASSWORD` and `session_secret` before production use.
- Prefer absolute `ROPDS_LIBRARY_ROOT` in production.
- Keep mounted config file read-only.
- Keep `session_secret` stable across restarts to preserve user sessions.
- `ROPDS_ADMIN_PASSWORD` is passed as a CLI argument during admin init and may be visible in the process list (`/proc/PID/cmdline`, `docker inspect`). For sensitive environments, run `ropds --set-admin` manually inside the container instead.

## Library layout note

- `covers_path` and `upload_path` defaults point inside `/library`.
- Cover settings are configured in `[covers]` (`covers_path`, `cover_max_dimension_px`, `cover_jpeg_quality`, `show_covers`).
- Keep only supported book formats in book folders to avoid scanning non-book files.

## Reverse proxy

For production HTTPS setup behind Nginx or Traefik, see:

- [`../service/proxy/README.md`](../service/proxy/README.md)
