#!/usr/bin/env python3
"""
Migrate full ROPDS data from SQLite to PostgreSQL or MySQL/MariaDB.

The script intentionally uses only Python stdlib. For the target database it
uses command-line clients:
  - PostgreSQL: psql
  - MySQL/MariaDB: mysql

Usage examples:
  python3 scripts/migrate_sqlite.py \
      --sqlite-db /path/to/ropds.db \
      --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds'

  python3 scripts/migrate_sqlite.py \
      --sqlite-db /path/to/ropds.db \
      --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds'

  # Use psql/mysql from running DB container (no host DB client install):
  python3 scripts/migrate_sqlite.py \
      --sqlite-db /path/to/ropds.db \
      --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds' \
      --db-container ropds-postgres
"""

from __future__ import annotations

import argparse
import logging
import math
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, TextIO
from urllib.parse import unquote, urlparse


LOG = logging.getLogger("sqlite_migrate")
DEFAULT_EXCLUDED_TABLES = {"_sqlx_migrations"}


class MigrationError(RuntimeError):
    """Raised on migration failure."""


@dataclass(frozen=True)
class TargetDsn:
    backend: str  # "postgres" or "mysql"
    host: str
    port: int
    database: str
    user: str
    password: str


@dataclass(frozen=True)
class ContainerExec:
    runtime: str
    container: str


def setup_logging(level: str) -> None:
    logging.basicConfig(
        level=getattr(logging, level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(message)s",
    )


def parse_target_dsn(url: str) -> TargetDsn:
    parsed = urlparse(url)
    scheme = (parsed.scheme or "").lower()
    if scheme in ("postgres", "postgresql"):
        backend = "postgres"
        default_port = 5432
    elif scheme in ("mysql", "mariadb"):
        backend = "mysql"
        default_port = 3306
    else:
        raise MigrationError(
            f"Unsupported target URL scheme '{scheme}'. Use postgres:// or mysql://"
        )

    if not parsed.hostname:
        raise MigrationError("Target URL must include hostname")
    if not parsed.path or parsed.path == "/":
        raise MigrationError("Target URL must include database name in path")
    if parsed.username is None:
        raise MigrationError("Target URL must include username")

    return TargetDsn(
        backend=backend,
        host=parsed.hostname,
        port=parsed.port or default_port,
        database=unquote(parsed.path.lstrip("/")),
        user=unquote(parsed.username),
        password=unquote(parsed.password or ""),
    )


def sqlite_quote_ident(name: str) -> str:
    return '"' + name.replace('"', '""') + '"'


class TargetClientBase:
    def __init__(
        self,
        dsn: TargetDsn,
        max_statement_bytes: int,
        container_exec: ContainerExec | None = None,
    ):
        self.dsn = dsn
        self.max_statement_bytes = max_statement_bytes
        self.container_exec = container_exec

    def check_client_binary(self) -> None:
        raise NotImplementedError

    def execute(self, sql: str) -> None:
        raise NotImplementedError

    def query(self, sql: str) -> list[list[str]]:
        raise NotImplementedError

    def run_sql_file(self, path: Path) -> None:
        raise NotImplementedError

    def quote_ident(self, ident: str) -> str:
        raise NotImplementedError

    def format_literal(self, value: object) -> str:
        raise NotImplementedError

    def list_tables(self) -> list[str]:
        raise NotImplementedError

    def table_columns(self, table: str) -> list[str]:
        raise NotImplementedError

    def table_row_count(self, table: str) -> int:
        q_table = self.quote_ident(table)
        rows = self.query(f"SELECT COUNT(*) FROM {q_table}")
        if not rows:
            raise MigrationError(f"Failed to get row count for target table {table}")
        return int(rows[0][0])

    def migration_versions(self) -> list[int]:
        if "_sqlx_migrations" not in self.list_tables():
            return []
        rows = self.query("SELECT version FROM _sqlx_migrations ORDER BY version")
        return [int(r[0]) for r in rows]

    def start_bulk_mode(self) -> None:
        raise NotImplementedError

    def finish_bulk_mode(self) -> None:
        raise NotImplementedError

    def truncate_tables(self, tables: list[str]) -> None:
        raise NotImplementedError

    def reset_sequences(self, tables: list[str]) -> None:
        # Most backends don't need explicit reset.
        return

    def write_insert_statements(
        self,
        out: TextIO,
        table: str,
        columns: list[str],
        rows: Iterable[tuple[object, ...]],
    ) -> int:
        q_table = self.quote_ident(table)
        q_cols = ", ".join(self.quote_ident(c) for c in columns)
        prefix = f"INSERT INTO {q_table} ({q_cols}) VALUES "
        rows_written = 0
        current_rows: list[str] = []
        current_size = len(prefix) + 1
        for row in rows:
            row_sql = "(" + ", ".join(self.format_literal(v) for v in row) + ")"
            row_size = len(row_sql) + 2
            if current_rows and (current_size + row_size > self.max_statement_bytes):
                out.write(prefix)
                out.write(", ".join(current_rows))
                out.write(";\n")
                current_rows = [row_sql]
                current_size = len(prefix) + len(row_sql) + 1
            else:
                current_rows.append(row_sql)
                current_size += row_size
            rows_written += 1
        if current_rows:
            out.write(prefix)
            out.write(", ".join(current_rows))
            out.write(";\n")
        return rows_written


class PostgresClient(TargetClientBase):
    def __init__(
        self,
        dsn: TargetDsn,
        max_statement_bytes: int,
        container_exec: ContainerExec | None = None,
    ):
        super().__init__(dsn, max_statement_bytes, container_exec)
        psql_args = [
            "psql",
            "-X",
            "-v",
            "ON_ERROR_STOP=1",
            "-h",
            dsn.host,
            "-p",
            str(dsn.port),
            "-U",
            dsn.user,
            "-d",
            dsn.database,
        ]
        if self.container_exec:
            self.base_args = [
                self.container_exec.runtime,
                "exec",
                "-i",
                "-e",
                f"PGPASSWORD={dsn.password}",
                self.container_exec.container,
                *psql_args,
            ]
            self.env = None
        else:
            self.base_args = psql_args
            self.env = os.environ.copy()
            self.env["PGPASSWORD"] = dsn.password

    def check_client_binary(self) -> None:
        if self.container_exec:
            if shutil.which(self.container_exec.runtime) is None:
                raise MigrationError(
                    f"{self.container_exec.runtime} not found in PATH"
                )
            probe = [
                self.container_exec.runtime,
                "exec",
                "-i",
                self.container_exec.container,
                "psql",
                "--version",
            ]
            proc = subprocess.run(
                probe,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            if proc.returncode != 0:
                err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
                raise MigrationError(
                    "Unable to execute psql inside container "
                    f"'{self.container_exec.container}': {err}"
                )
            return
        if shutil.which("psql") is None:
            raise MigrationError("psql not found in PATH")

    def _run(self, args: list[str], input_text: str | None = None) -> str:
        proc = subprocess.run(
            args,
            input=input_text,
            text=True,
            env=self.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if proc.returncode != 0:
            err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
            raise MigrationError(f"psql failed: {err}")
        return proc.stdout

    def execute(self, sql: str) -> None:
        self._run(self.base_args + ["-q", "-c", sql])

    def query(self, sql: str) -> list[list[str]]:
        out = self._run(self.base_args + ["-A", "-F", "\t", "-t", "-c", sql])
        rows: list[list[str]] = []
        for line in out.splitlines():
            line = line.rstrip("\n")
            if not line:
                continue
            rows.append(line.split("\t"))
        return rows

    def run_sql_file(self, path: Path) -> None:
        if self.container_exec:
            with path.open("r", encoding="utf-8") as f:
                proc = subprocess.run(
                    self.base_args + ["-q"],
                    stdin=f,
                    text=True,
                    env=self.env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
            if proc.returncode != 0:
                err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
                raise MigrationError(f"psql failed: {err}")
            return
        self._run(self.base_args + ["-q", "-f", str(path)])

    def quote_ident(self, ident: str) -> str:
        return '"' + ident.replace('"', '""') + '"'

    def format_literal(self, value: object) -> str:
        if value is None:
            return "NULL"
        if isinstance(value, bool):
            return "TRUE" if value else "FALSE"
        if isinstance(value, int):
            return str(value)
        if isinstance(value, float):
            if not math.isfinite(value):
                raise MigrationError("Non-finite float value is not supported")
            return repr(value)
        if isinstance(value, bytes):
            return f"'\\\\x{value.hex()}'::bytea"
        if isinstance(value, str):
            if "\x00" in value:
                raise MigrationError("NUL byte in text is not supported by PostgreSQL")
            return "'" + value.replace("'", "''") + "'"
        raise MigrationError(f"Unsupported value type: {type(value)!r}")

    def list_tables(self) -> list[str]:
        rows = self.query(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = 'public' AND table_type = 'BASE TABLE' "
            "ORDER BY table_name"
        )
        return [r[0] for r in rows]

    def table_columns(self, table: str) -> list[str]:
        safe_table = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name FROM information_schema.columns "
            f"WHERE table_schema = 'public' AND table_name = '{safe_table}' "
            "ORDER BY ordinal_position"
        )
        return [r[0] for r in rows]

    def start_bulk_mode(self) -> None:
        self.execute("SET session_replication_role = replica")

    def finish_bulk_mode(self) -> None:
        self.execute("SET session_replication_role = origin")

    def truncate_tables(self, tables: list[str]) -> None:
        if not tables:
            return
        quoted = ", ".join(self.quote_ident(t) for t in tables)
        self.execute(f"TRUNCATE TABLE {quoted} RESTART IDENTITY CASCADE")

    def reset_sequences(self, tables: list[str]) -> None:
        for table in tables:
            safe_table = table.replace("'", "''")
            cols = self.query(
                "SELECT column_name FROM information_schema.columns "
                "WHERE table_schema = 'public' "
                f"AND table_name = '{safe_table}' "
                "AND column_default LIKE 'nextval%'"
            )
            for (col,) in cols:
                q_table = self.quote_ident(table)
                q_col = self.quote_ident(col)
                safe_col = col.replace("'", "''")
                self.execute(
                    "SELECT setval("
                    f"pg_get_serial_sequence('{safe_table}', '{safe_col}'), "
                    f"COALESCE((SELECT MAX({q_col}) FROM {q_table}), 0) + 1, "
                    "false)"
                )


class MysqlClient(TargetClientBase):
    def __init__(
        self,
        dsn: TargetDsn,
        max_statement_bytes: int,
        container_exec: ContainerExec | None = None,
    ):
        super().__init__(dsn, max_statement_bytes, container_exec)
        self.cli_bin = "mysql"
        mysql_args = [
            self.cli_bin,
            "--batch",
            "--raw",
            "--skip-column-names",
            "-h",
            dsn.host,
            "-P",
            str(dsn.port),
            "-u",
            dsn.user,
            dsn.database,
        ]
        if dsn.password:
            mysql_args.insert(-1, f"-p{dsn.password}")
        if self.container_exec:
            self.base_args = [
                self.container_exec.runtime,
                "exec",
                "-i",
                self.container_exec.container,
                *mysql_args,
            ]
        else:
            self.base_args = mysql_args

    def _select_container_cli_bin(self) -> str:
        assert self.container_exec is not None
        for candidate in ("mysql", "mariadb"):
            probe = [
                self.container_exec.runtime,
                "exec",
                "-i",
                self.container_exec.container,
                candidate,
                "--version",
            ]
            proc = subprocess.run(
                probe,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            if proc.returncode == 0:
                return candidate
        return ""

    def check_client_binary(self) -> None:
        if self.container_exec:
            if shutil.which(self.container_exec.runtime) is None:
                raise MigrationError(
                    f"{self.container_exec.runtime} not found in PATH"
                )
            selected = self._select_container_cli_bin()
            if not selected:
                raise MigrationError(
                    "Unable to execute mysql/mariadb client inside container "
                    f"'{self.container_exec.container}'"
                )
            if selected != self.cli_bin:
                self.cli_bin = selected
                self.base_args = [selected if a == "mysql" else a for a in self.base_args]
            return
        if shutil.which("mysql") is not None:
            return
        if shutil.which("mariadb") is not None:
            self.cli_bin = "mariadb"
            self.base_args = [self.cli_bin if a == "mysql" else a for a in self.base_args]
            return
        raise MigrationError("mysql/mariadb client not found in PATH")

    def _run(self, args: list[str], input_text: str | None = None) -> str:
        proc = subprocess.run(
            args,
            input=input_text,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if proc.returncode != 0:
            err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
            raise MigrationError(f"mysql failed: {err}")
        return proc.stdout

    def execute(self, sql: str) -> None:
        self._run(self.base_args + ["-e", sql])

    def query(self, sql: str) -> list[list[str]]:
        out = self._run(self.base_args + ["-e", sql])
        rows: list[list[str]] = []
        for line in out.splitlines():
            line = line.rstrip("\n")
            if not line:
                continue
            rows.append(line.split("\t"))
        return rows

    def run_sql_file(self, path: Path) -> None:
        with path.open("r", encoding="utf-8") as f:
            proc = subprocess.run(
                self.base_args,
                stdin=f,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
        if proc.returncode != 0:
            err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
            raise MigrationError(f"mysql failed: {err}")

    def quote_ident(self, ident: str) -> str:
        return "`" + ident.replace("`", "``") + "`"

    def format_literal(self, value: object) -> str:
        if value is None:
            return "NULL"
        if isinstance(value, bool):
            return "1" if value else "0"
        if isinstance(value, int):
            return str(value)
        if isinstance(value, float):
            if not math.isfinite(value):
                raise MigrationError("Non-finite float value is not supported")
            return repr(value)
        if isinstance(value, bytes):
            return f"X'{value.hex()}'"
        if isinstance(value, str):
            hexval = value.encode("utf-8").hex()
            return f"CONVERT(X'{hexval}' USING utf8mb4)"
        raise MigrationError(f"Unsupported value type: {type(value)!r}")

    def list_tables(self) -> list[str]:
        rows = self.query(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' "
            "ORDER BY table_name"
        )
        return [r[0] for r in rows]

    def table_columns(self, table: str) -> list[str]:
        safe_table = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name FROM information_schema.columns "
            "WHERE table_schema = DATABASE() "
            f"AND table_name = '{safe_table}' "
            "ORDER BY ordinal_position"
        )
        return [r[0] for r in rows]

    def start_bulk_mode(self) -> None:
        # mysql client calls are session-scoped; this hook is intentionally no-op.
        return

    def finish_bulk_mode(self) -> None:
        # mysql client calls are session-scoped; this hook is intentionally no-op.
        return

    def truncate_tables(self, tables: list[str]) -> None:
        if not tables:
            return
        parts = ["SET FOREIGN_KEY_CHECKS = 0"]
        parts.extend(f"TRUNCATE TABLE {self.quote_ident(table)}" for table in tables)
        parts.append("SET FOREIGN_KEY_CHECKS = 1")
        self.execute("; ".join(parts))


def make_target_client(
    dsn: TargetDsn,
    max_statement_bytes: int,
    container_exec: ContainerExec | None = None,
) -> TargetClientBase:
    if dsn.backend == "postgres":
        return PostgresClient(dsn, max_statement_bytes, container_exec)
    if dsn.backend == "mysql":
        return MysqlClient(dsn, max_statement_bytes, container_exec)
    raise MigrationError(f"Unsupported backend: {dsn.backend}")


def sqlite_tables(conn: sqlite3.Connection) -> list[str]:
    rows = conn.execute(
        "SELECT name FROM sqlite_master "
        "WHERE type = 'table' AND name NOT LIKE 'sqlite_%' "
        "ORDER BY name"
    ).fetchall()
    return [r[0] for r in rows]


def sqlite_columns(conn: sqlite3.Connection, table: str) -> list[str]:
    q_table = sqlite_quote_ident(table)
    rows = conn.execute(f"PRAGMA table_info({q_table})").fetchall()
    return [r[1] for r in rows]


def sqlite_migration_versions(conn: sqlite3.Connection) -> list[int]:
    if "_sqlx_migrations" not in sqlite_tables(conn):
        return []
    rows = conn.execute("SELECT version FROM _sqlx_migrations ORDER BY version").fetchall()
    return [int(r[0]) for r in rows]


def sqlite_foreign_keys(conn: sqlite3.Connection, table: str) -> list[str]:
    q_table = sqlite_quote_ident(table)
    rows = conn.execute(f"PRAGMA foreign_key_list({q_table})").fetchall()
    # PRAGMA foreign_key_list columns: id, seq, table, from, to, ...
    return [r[2] for r in rows]


def topological_order(conn: sqlite3.Connection, tables: list[str]) -> list[str]:
    table_set = set(tables)
    deps: dict[str, set[str]] = {t: set() for t in tables}
    reverse: dict[str, set[str]] = {t: set() for t in tables}

    for t in tables:
        for dep in sqlite_foreign_keys(conn, t):
            if dep in table_set and dep != t:
                deps[t].add(dep)
                reverse[dep].add(t)

    incoming = {t: len(deps[t]) for t in tables}
    ready = sorted(t for t, cnt in incoming.items() if cnt == 0)
    ordered: list[str] = []

    while ready:
        cur = ready.pop(0)
        ordered.append(cur)
        for nxt in sorted(reverse[cur]):
            incoming[nxt] -= 1
            if incoming[nxt] == 0:
                ready.append(nxt)
                ready.sort()

    remaining = sorted(t for t, cnt in incoming.items() if cnt > 0)
    if remaining:
        LOG.warning(
            "Detected dependency cycles (likely self-references); appending in name order: %s",
            ", ".join(remaining),
        )
        ordered.extend(remaining)
    return ordered


def count_rows(conn: sqlite3.Connection, table: str) -> int:
    q_table = sqlite_quote_ident(table)
    row = conn.execute(f"SELECT COUNT(*) FROM {q_table}").fetchone()
    return int(row[0]) if row else 0


def iter_rows(
    conn: sqlite3.Connection,
    table: str,
    columns: list[str],
    batch_size: int,
) -> Iterable[list[tuple[object, ...]]]:
    q_table = sqlite_quote_ident(table)
    q_cols = ", ".join(sqlite_quote_ident(c) for c in columns)
    cur = conn.execute(f"SELECT {q_cols} FROM {q_table}")
    while True:
        chunk = cur.fetchmany(batch_size)
        if not chunk:
            break
        yield chunk


def migrate(args: argparse.Namespace) -> None:
    sqlite_path = Path(args.sqlite_db).expanduser()
    if not sqlite_path.exists():
        raise MigrationError(f"SQLite DB not found: {sqlite_path}")

    dsn = parse_target_dsn(args.target_url)
    container_exec = None
    if args.db_container:
        container_exec = ContainerExec(
            runtime=args.container_runtime,
            container=args.db_container,
        )
        LOG.info(
            "Target DB client mode: %s exec in container '%s'",
            container_exec.runtime,
            container_exec.container,
        )

    client = make_target_client(
        dsn,
        max_statement_bytes=args.max_statement_bytes,
        container_exec=container_exec,
    )
    client.check_client_binary()

    LOG.info("Opening SQLite DB: %s", sqlite_path)
    conn = sqlite3.connect(f"file:{sqlite_path}?mode=ro", uri=True)
    conn.row_factory = None

    try:
        source_tables = sqlite_tables(conn)
        excluded = set(DEFAULT_EXCLUDED_TABLES)
        if args.include_sqlx_migrations:
            excluded.discard("_sqlx_migrations")
        source_tables = [t for t in source_tables if t not in excluded]
        if not source_tables:
            raise MigrationError("No source tables found in SQLite database")

        target_tables = set(client.list_tables())
        source_versions = sqlite_migration_versions(conn)
        target_versions = client.migration_versions()
        if source_versions and not target_versions and not args.include_sqlx_migrations:
            raise MigrationError(
                "Target DB has no _sqlx_migrations entries. "
                "Run ROPDS once against target DB to apply migrations first."
            )
        if source_versions and target_versions and not args.include_sqlx_migrations:
            src_set = set(source_versions)
            dst_set = set(target_versions)
            missing_versions = sorted(v for v in src_set if v not in dst_set)
            if missing_versions:
                raise MigrationError(
                    "Target DB is missing migration versions present in source: "
                    + ", ".join(str(v) for v in missing_versions)
                    + ". Update target schema before migration."
                )
        missing = [t for t in source_tables if t not in target_tables]
        if missing:
            raise MigrationError(
                "Target DB is missing required tables: "
                + ", ".join(sorted(missing))
                + ". Run ROPDS once against target DB to apply migrations."
            )

        ordered_tables = topological_order(conn, source_tables)
        LOG.info("Will migrate %d tables", len(ordered_tables))

        table_columns_map: dict[str, list[str]] = {}
        for table in ordered_tables:
            src_cols = sqlite_columns(conn, table)
            dst_cols = client.table_columns(table)
            src_set = set(src_cols)
            dst_set = set(dst_cols)
            missing = [c for c in src_cols if c not in dst_set]
            if missing:
                raise MigrationError(
                    f"Table '{table}' schema mismatch; target is missing columns: "
                    + ", ".join(missing)
                )
            extra = [c for c in dst_cols if c not in src_set]
            if extra:
                LOG.info(
                    "Table %s: target has additional columns (defaults will apply): %s",
                    table,
                    ", ".join(extra),
                )
            cols = src_cols
            table_columns_map[table] = cols

        if args.truncate_target:
            LOG.info("Truncating target tables before load")
            client.start_bulk_mode()
            try:
                client.truncate_tables(ordered_tables)
            finally:
                client.finish_bulk_mode()

        total_rows = 0
        started = time.monotonic()

        client.start_bulk_mode()
        try:
            for idx, table in enumerate(ordered_tables, start=1):
                cols = table_columns_map[table]
                table_total = count_rows(conn, table)
                LOG.info("[%d/%d] %s: %d rows", idx, len(ordered_tables), table, table_total)
                if table_total == 0:
                    continue

                migrated = 0
                with tempfile.NamedTemporaryFile(
                    mode="w",
                    encoding="utf-8",
                    suffix=f".{dsn.backend}.sql",
                    prefix=f"ropds_migrate_{table}_",
                    delete=False,
                ) as tmp:
                    script_path = Path(tmp.name)

                try:
                    with script_path.open("w", encoding="utf-8") as out:
                        for batch in iter_rows(conn, table, cols, args.fetch_batch_size):
                            written = client.write_insert_statements(out, table, cols, batch)
                            migrated += written
                            if migrated % args.progress_every_rows == 0:
                                LOG.info(
                                    "%s: migrated %d/%d rows",
                                    table,
                                    migrated,
                                    table_total,
                                )

                    client.run_sql_file(script_path)
                finally:
                    script_path.unlink(missing_ok=True)

                total_rows += migrated
                LOG.info("%s: done (%d rows)", table, migrated)
        finally:
            client.finish_bulk_mode()

        if dsn.backend == "postgres":
            LOG.info("Resetting PostgreSQL sequences")
            client.reset_sequences(ordered_tables)

        LOG.info("Verifying row counts in target")
        for table in ordered_tables:
            src_count = count_rows(conn, table)
            dst_count = client.table_row_count(table)
            if src_count != dst_count:
                raise MigrationError(
                    f"Row count mismatch for table '{table}': source={src_count}, target={dst_count}"
                )
            LOG.info("Verified %s: %d rows", table, dst_count)

        elapsed = time.monotonic() - started
        LOG.info(
            "Migration completed: %d tables, %d rows, %.1f seconds",
            len(ordered_tables),
            total_rows,
            elapsed,
        )
    finally:
        conn.close()


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Migrate ROPDS data from SQLite to PostgreSQL or MySQL/MariaDB"
    )
    parser.add_argument(
        "--sqlite-db",
        required=True,
        help="Path to source SQLite database file",
    )
    parser.add_argument(
        "--target-url",
        required=True,
        help=(
            "Target DB URL: postgres://user:pass@host:port/db or "
            "mysql://user:pass@host:port/db"
        ),
    )
    parser.add_argument(
        "--db-container",
        help=(
            "Run target DB CLI inside this container using '<container-runtime> exec -i'. "
            "Useful when host doesn't have psql/mysql installed."
        ),
    )
    parser.add_argument(
        "--container-runtime",
        default="docker",
        help="Container runtime binary for --db-container mode (default: docker)",
    )
    parser.add_argument(
        "--truncate-target",
        action="store_true",
        default=True,
        help="Truncate target tables before importing data (default: enabled)",
    )
    parser.add_argument(
        "--no-truncate-target",
        action="store_false",
        dest="truncate_target",
        help="Do not truncate target tables before import",
    )
    parser.add_argument(
        "--include-sqlx-migrations",
        action="store_true",
        help="Also migrate _sqlx_migrations table (off by default)",
    )
    parser.add_argument(
        "--fetch-batch-size",
        type=int,
        default=500,
        help="Rows fetched from SQLite per batch (default: 500)",
    )
    parser.add_argument(
        "--max-statement-bytes",
        type=int,
        default=4 * 1024 * 1024,
        help="Max generated INSERT statement size in bytes (default: 4 MiB)",
    )
    parser.add_argument(
        "--progress-every-rows",
        type=int,
        default=50000,
        help="Progress log interval in rows per table (default: 50000)",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
        help="Log level (default: INFO)",
    )
    args = parser.parse_args(argv)
    if args.fetch_batch_size < 1:
        parser.error("--fetch-batch-size must be > 0")
    if args.max_statement_bytes < 1024:
        parser.error("--max-statement-bytes must be >= 1024")
    if args.progress_every_rows < 1:
        parser.error("--progress-every-rows must be > 0")
    return args


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    setup_logging(args.log_level)
    try:
        migrate(args)
        return 0
    except KeyboardInterrupt:
        LOG.error("Migration interrupted by user")
        return 130
    except MigrationError as e:
        LOG.error("Migration failed: %s", e)
        return 1
    except Exception:  # pragma: no cover - top-level safety net
        LOG.exception("Unexpected migration failure")
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
