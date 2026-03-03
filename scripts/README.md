# SQLite -> PostgreSQL / MySQL(MariaDB) Migration Guide

This directory contains `migrate_sqlite.py` for full ROPDS data migration from SQLite to PostgreSQL or MySQL/MariaDB.

## What The Script Does

- Reads all SQLite tables (except `_sqlx_migrations` by default).
- Validates target schema and migration versions.
- Truncates target tables before import (default behavior).
- Imports table data with dependency-aware order.
- Verifies row counts table-by-table after import.

The script uses Python stdlib only.  
For target DB access it runs `psql` or `mysql`/`mariadb` CLI.

## Prerequisites

1. Source SQLite DB file exists (example: `devel/ropds.db`).
2. Target DB server is reachable.
3. Target DB schema is already initialized by ROPDS migrations.

To initialize target schema, run ROPDS once with target DB URL:

```bash
./target/debug/ropds -c /path/to/config.toml --set-admin your-password
```

## 1) PostgreSQL Migration (Host CLI)

1. Stop running ROPDS instance (recommended during migration).
2. Make SQLite backup:

```bash
cp /path/to/ropds.db /path/to/ropds.db.bak.$(date +%F-%H%M%S)
```

3. Run migration:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds'
```

4. Start ROPDS with PostgreSQL URL in config.

## 2) MariaDB/MySQL Migration (Host CLI)

1. Stop running ROPDS instance (recommended).
2. Make SQLite backup.
3. Run migration:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds'
```

4. Start ROPDS with MySQL URL in config.

## 3) If DB Runs In Container (No Host DB CLI Installed)

Use direct container mode:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds' \
  --db-container ropds-postgres \
  --container-runtime docker
```

MariaDB example:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds' \
  --db-container ropds-mariadb \
  --container-runtime docker
```

For Podman:

```bash
--container-runtime podman
```

## Verification Checklist

After migration:

1. Start ROPDS with target DB config.
2. Open `/health` and `/web`.
3. Verify key counts:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

4. Open reader and confirm:
- `POST /web/api/reading-position` returns `{"ok": true}`
- `/web/api/reading-history` returns expected entries.

## Useful Options

- `--no-truncate-target` - keep existing target rows (advanced use).
- `--include-sqlx-migrations` - also migrate `_sqlx_migrations`.
- `--fetch-batch-size N` - source fetch chunk size (default `500`).
- `--max-statement-bytes N` - max generated SQL statement size.
- `--progress-every-rows N` - progress log interval.
- `--log-level DEBUG|INFO|WARNING|ERROR`

Show all options:

```bash
python3 scripts/migrate_sqlite.py --help
```

## Notes

- Target schema mismatch or missing migrations cause a hard fail by design.
- Script default is "safe overwrite" of target data (truncate + full import + row-count verification).
- Keep `library.root_path` in target config consistent with book paths stored in DB.
