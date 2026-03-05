# Migration guide: SQLite -> PostgreSQL / MySQL (MariaDB)

This directory contains `migrate_sqlite.py` — a script that transfers all ROPDS data from SQLite to PostgreSQL or MySQL/MariaDB.

## What the script does

- Reads every SQLite table (except `_sqlx_migrations` by default)
- Validates the target schema and migration versions
- Truncates target tables before import (default behavior)
- Imports data in dependency-aware order
- Verifies row counts table by table after import

The script uses only the Python standard library. For the target database, it shells out to `psql` or `mysql`/`mariadb`.

## Prerequisites

1. You have the source SQLite database file (e.g. `devel/ropds.db`).
2. The target database server is reachable.
3. The target schema is already initialized by ROPDS migrations.

To initialize the schema, run ROPDS once with the target DB URL in `config.toml`:

```bash
./target/debug/ropds -c /path/to/config.toml --set-admin your-password
```

## 1) PostgreSQL (host CLI)

1. Stop the running ROPDS instance (recommended during migration).
2. Back up the SQLite file:

```bash
cp /path/to/ropds.db /path/to/ropds.db.bak.$(date +%F-%H%M%S)
```

3. Run the migration:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds'
```

4. Start ROPDS with the PostgreSQL URL in your config.

## 2) MySQL / MariaDB (host CLI)

1. Stop the running ROPDS instance.
2. Back up the SQLite file.
3. Run the migration:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds'
```

4. Start ROPDS with the MySQL URL in your config.

## 3) When the database runs in a container (no host CLI)

If you don't have `psql` or `mysql` installed on the host, point the script at the container directly:

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

## Post-migration checklist

1. Start ROPDS with the target database config.
2. Open `/health` and `/web` — make sure both respond.
3. Spot-check key row counts:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

4. Open the embedded reader and confirm:
- `POST /web/api/reading-position` returns `{"ok": true}`
- `/web/api/reading-history` returns expected entries

## Additional options

- `--no-truncate-target` — keep existing rows in the target (advanced use)
- `--include-sqlx-migrations` — also migrate the `_sqlx_migrations` table
- `--fetch-batch-size N` — source fetch chunk size (default `500`)
- `--max-statement-bytes N` — maximum generated SQL statement size
- `--progress-every-rows N` — progress log interval
- `--log-level DEBUG|INFO|WARNING|ERROR`

Full list:

```bash
python3 scripts/migrate_sqlite.py --help
```

## Notes

- Schema mismatch or missing migrations cause a hard fail — by design.
- Default mode is "safe overwrite": truncate target tables, full import, then row-count verification.
- Keep `library.root_path` in the new config consistent with the book paths stored in the database.
