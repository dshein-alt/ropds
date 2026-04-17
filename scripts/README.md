# Migration guide: SQLite -> PostgreSQL or MySQL/MariaDB

**Supported target versions** (tested end-to-end):

| Backend    | Minimum | Notes                                                          |
|------------|---------|----------------------------------------------------------------|
| PostgreSQL | 16+     | Tested on 16 and 17.                                           |
| MariaDB    | 11+     | Tested on 11.x and 12.x.                                       |
| MySQL      | 8+      | Tested on 8.4. Default `ONLY_FULL_GROUP_BY` SQL mode is fine.  |

Migrating an existing SQLite ROPDS database to PostgreSQL, MySQL, or MariaDB is a four-step flow:

1. **Create the role and database** on the target server (one-time).
2. **Initialize the schema with `ropds --init-db`**.
3. **Copy the data with `scripts/migrate_sqlite.py`**.
4. **Start ROPDS against the new database.**

The migration script is intentionally narrow — it only copies rows. Schema creation, migration bookkeeping, and preflight safety live in the ROPDS binary.

---

## 1. Create the role and database

### PostgreSQL

Connect as a role that has `CREATEROLE` and `CREATEDB` (typically `postgres`) and run:

```sql
-- Replace 'strongpassword' with a real password.
CREATE USER ropds WITH LOGIN PASSWORD 'strongpassword';

CREATE DATABASE ropds
    OWNER ropds
    ENCODING 'UTF8'
    TEMPLATE template0
    LC_COLLATE 'C.UTF-8'
    LC_CTYPE  'C.UTF-8';

\c ropds
GRANT ALL ON SCHEMA public TO ropds;
ALTER SCHEMA public OWNER TO ropds;
```

For remote access, add an entry to `pg_hba.conf` and reload:

```
host    ropds    ropds    10.0.0.0/24    scram-sha-256
```

### MySQL / MariaDB

Connect as `root` and run:

```sql
CREATE DATABASE ropds CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- '%' lets the role connect from any host; restrict as appropriate.
CREATE USER 'ropds'@'%' IDENTIFIED BY 'strongpassword';
GRANT ALL PRIVILEGES ON ropds.* TO 'ropds'@'%';
FLUSH PRIVILEGES;
```

**The migration role does not need to be a PostgreSQL superuser or MySQL `root`.** Ownership of the target database is all that is required.

---

## 2. Prepare the schema with `ropds --init-db`

Point the `[database].url` in your ROPDS config at the target, then run:

```bash
cargo run -- --config=config.toml --init-db
# or, against a built binary:
./target/release/ropds --config=config.toml --init-db
```

What this does:

- Connects to the target using the URL from config. If the DB does not exist and the role can create it, creates it.
- Preflight safety check: if the target already has ROPDS tables but no `_sqlx_migrations`, refuses and exits non-zero.
- Applies every embedded migration in order (idempotent — already-applied migrations are skipped).
- **Clears every user table** so the target is truly empty of data. Seed rows the migrations would normally insert (genres, counters, etc.) are wiped — they will be restored from the SQLite source during step 3.
- Exits 0.

After this step the target has the full schema, `_sqlx_migrations` with correct backend-specific checksums, and zero data rows.

> **`--init-db` is migration-prep, not a fresh-install flag.** For a fresh install with no SQLite source, just start the server normally; that path applies migrations and leaves seed data in place.
>
> **Safety**: if any data table already contains rows, `--init-db` refuses and exits with an error listing the populated tables and the required manual step — truncate those tables (or drop and recreate the database) before retrying. This means `--init-db` is safe to accidentally invoke against a live or already-migrated database.

---

## 3. Copy the data

### Local client

```bash
# PostgreSQL
python3 scripts/migrate_sqlite.py \
    /path/to/ropds.db \
    'postgres://ropds:strongpassword@db.example.com:5432/ropds'

# MySQL / MariaDB
python3 scripts/migrate_sqlite.py \
    /path/to/ropds.db \
    'mysql://ropds:strongpassword@db.example.com:3306/ropds'
```

The URL scheme picks the backend (`postgres://` / `postgresql://` or `mysql://` / `mariadb://`).

### DB runs inside a container

If the target is a containerized DB and the host has no `psql`/`mysql` installed, route the client through the running container:

```bash
# docker
python3 scripts/migrate_sqlite.py \
    --db-container ropds-postgres --container-runtime docker \
    /path/to/ropds.db \
    'postgres://ropds:strongpassword@127.0.0.1:5432/ropds'

# podman
python3 scripts/migrate_sqlite.py \
    --db-container ropds-mariadb --container-runtime podman \
    /path/to/ropds.db \
    'mysql://ropds:strongpassword@127.0.0.1:3306/ropds'
```

When `--db-container` is given the script runs `<runtime> exec -i <container> psql|mysql …`, so the port/host in the URL is the one the DB listens on **inside the container** (typically 5432 / 3306), not any host-side forwarded port.

### What the script does

1. Opens the SQLite source read-only.
2. Verifies every source data table exists in the target and every source column exists in the target column set.
3. **Emptiness precheck**: every target data table must have 0 rows (the state `ropds --init-db` leaves behind). If any table is non-empty, the script lists the offenders and refuses — so you cannot silently truncate over an already-populated DB.
4. Prompts `Proceed? [y/N]:`. Answer `y` to continue. Piped stdin is refused so a destructive operation cannot be silently scripted.
5. Builds a single SQL script containing truncation + batched INSERTs (with rows inside self-referential tables sorted parent-before-child) + sequence/auto-increment resets, and runs it in one CLI session:
   - **PostgreSQL**: `BEGIN; TRUNCATE … RESTART IDENTITY CASCADE; INSERT …; SELECT setval(…); COMMIT;` — atomic; a failure rolls back.
   - **MySQL/MariaDB**: `SET FOREIGN_KEY_CHECKS = 0; TRUNCATE TABLE … ; INSERT …; ALTER TABLE … AUTO_INCREMENT = …; SET FOREIGN_KEY_CHECKS = 1;` — note that MySQL `TRUNCATE` auto-commits, so the load is **not** atomic; on failure the target may be left in a partial state and you should re-run.
6. Verifies the row count of every table against the source and logs the result.

Because the entire destructive sequence is one CLI session, `SET FOREIGN_KEY_CHECKS = 0` / transaction semantics apply to every statement and there is no session-hopping hazard.

---

## 4. Start ROPDS

```bash
./target/release/ropds --config=config.toml
```

`sqlx` sees every migration already recorded with matching checksums; the freshly-copied data is intact; the server starts listening.

---

## Backing up before you migrate

```bash
cp /path/to/ropds.db /path/to/ropds.db.bak.$(date +%F-%H%M%S)
```

## Post-migration checklist

1. Open `/health` and `/web` — confirm both respond.
2. Spot-check key row counts:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

3. Open the embedded reader and confirm `POST /web/api/reading-position` returns `{"ok": true}` and `/web/api/reading-history` has expected entries.

## Notes

- `_sqlx_migrations` is never copied or altered by the script; it is owned by `ropds --init-db`.
- Running the migration script twice is idempotent when you re-run `ropds --init-db` between attempts: init-db clears the target, the precheck passes, and the script re-copies. Running the script a second time without re-init will fail the 0-rows precheck (because the first run populated the target).
- Schema mismatches (a source column missing in the target) cause a hard fail by design.
- Keep `library.root_path` in the new config consistent with the book paths stored in the database.
