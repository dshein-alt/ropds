# ROPDS Docker Deployment

This folder contains a complete Docker deployment bundle for ROPDS with base + override compose files.

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
docker compose \
  -f docker/docker-compose.yml \
  -f docker/docker-compose.sqlite.yml \
  up -d --build
```

3. Open:

- Web UI: `http://localhost:8081/web`
- OPDS: `http://localhost:8081/opds`

## Compose file matrix

| Scenario | Command |
|---|---|
| SQLite (volume-backed DB) | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.sqlite.yml up -d --build` |
| PostgreSQL sibling stack | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.postgres.sibling.yml up -d --build` |
| PostgreSQL external DB | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.postgres.external.yml up -d --build` |
| MySQL/MariaDB sibling stack | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.mysql.sibling.yml up -d --build` |
| MySQL/MariaDB external DB | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.mysql.external.yml up -d --build` |

## Sibling DB stacks

Start PostgreSQL sibling DB:

```bash
docker compose -f docker/db/postgres/docker-compose.yml up -d
```

Start MariaDB sibling DB:

```bash
docker compose -f docker/db/mysql/docker-compose.yml up -d
```

Then start ROPDS with corresponding sibling override.

## Config files

All app configs are in `docker/config/`:

- `config.toml.example`: valid Docker-ready default (SQLite), also used by the SQLite compose override
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

You can customize credentials/hosts directly in those files.

## Mount model

Base compose mounts:

- `./config/*.toml -> /app/config/config.toml` (read-only)
- `${ROPDS_LIBRARY_ROOT} -> /library` (read-write)

The image is self-contained and already includes:

- `/app/templates`
- `/app/locales`
- `/app/static`

Optional host static override (read-only):

```bash
docker compose \
  -f docker/docker-compose.yml \
  -f docker/docker-compose.sqlite.yml \
  -f docker/docker-compose.static.mount.yml \
  up -d --build
```

Runtime creates/uses:

- `/library/covers`
- `/library/uploads`
- SQLite DB volume at `/var/lib/ropds/sqlite` (SQLite scenario)

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
- Keep only supported book formats in book folders to avoid scanning non-book files.
