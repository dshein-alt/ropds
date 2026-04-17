#!/usr/bin/env python3
"""
Copy ROPDS data from a SQLite database to a PostgreSQL or MySQL/MariaDB database.

The target schema must already be initialized (e.g. via `ropds --init-db`).
This script does not create tables, apply migrations, or alter schema — it only
copies rows. `_sqlx_migrations` is never touched.

Usage:
    migrate_sqlite.py [--db-container NAME] [--container-runtime {docker,podman}] \\
        SQLITE_DB TARGET_URL

Examples:
    # PostgreSQL
    migrate_sqlite.py ./devel/ropds.db \\
        'postgres://ropds:PASSWORD@db.example.com:5432/ropds'

    # MySQL / MariaDB
    migrate_sqlite.py ./devel/ropds.db \\
        'mysql://ropds:PASSWORD@db.example.com:3306/ropds'

    # Target DB runs in a container (no host psql/mysql):
    migrate_sqlite.py --db-container ropds-postgres --container-runtime podman \\
        ./devel/ropds.db 'postgres://ropds:PASSWORD@127.0.0.1:5432/ropds'

Requires:
    - SQLite source DB (read-only).
    - Either `psql` / `mysql` CLI on PATH, or a running container
      (`--db-container`) that has the client binary.
    - Interactive stdin for the confirmation prompt; the script is destructive
      (TRUNCATEs every target data table before loading) and will refuse to
      proceed without explicit confirmation.
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


LOG = logging.getLogger("migrate_sqlite")
EXCLUDED_TABLES = {"_sqlx_migrations"}
MAX_STATEMENT_BYTES = 4 * 1024 * 1024
FETCH_BATCH_SIZE = 500


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
    runtime: str  # "docker" or "podman"
    container: str


def parse_target_dsn(url: str) -> TargetDsn:
    parsed = urlparse(url)
    scheme = (parsed.scheme or "").lower()
    if scheme in ("postgres", "postgresql"):
        backend, default_port = "postgres", 5432
    elif scheme in ("mysql", "mariadb"):
        backend, default_port = "mysql", 3306
    else:
        raise MigrationError(
            f"Unsupported target URL scheme '{scheme}'. "
            "Use postgres:// or mysql://"
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


# ---------------------------------------------------------------------------
# Target clients
# ---------------------------------------------------------------------------


class TargetClient:
    """Shared subprocess plumbing; backend-specific SQL lives in subclasses."""

    cli_bin: str = ""  # set by subclass; may be replaced at runtime for mariadb

    def __init__(
        self, dsn: TargetDsn, container_exec: ContainerExec | None = None
    ) -> None:
        self.dsn = dsn
        self.container_exec = container_exec
        self._cli_args: list[str] = []  # populated by subclass
        self.env: dict[str, str] | None = None  # populated by subclass

    # ---- subprocess plumbing ----

    @property
    def base_args(self) -> list[str]:
        """Command prefix for invoking the DB CLI (optionally inside a container)."""
        if self.container_exec:
            env_args: list[str] = []
            for k, v in (self.env or {}).items():
                if k.startswith(("PG", "MYSQL")):
                    env_args += ["-e", f"{k}={v}"]
            return [
                self.container_exec.runtime,
                "exec",
                "-i",
                *env_args,
                self.container_exec.container,
                *self._cli_args,
            ]
        return list(self._cli_args)

    def _subprocess_env(self) -> dict[str, str] | None:
        if self.container_exec:
            return None
        merged = os.environ.copy()
        for k, v in (self.env or {}).items():
            merged[k] = v
        return merged

    def _run(self, args: list[str], stdin_path: Path | None = None) -> str:
        kwargs: dict = dict(
            env=self._subprocess_env(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
        )
        if stdin_path is not None:
            with stdin_path.open("r", encoding="utf-8") as f:
                proc = subprocess.run(args, stdin=f, **kwargs)
        else:
            proc = subprocess.run(args, **kwargs)
        if proc.returncode != 0:
            err = proc.stderr.strip() or proc.stdout.strip() or "unknown error"
            raise MigrationError(f"{self.cli_bin} failed: {err}")
        return proc.stdout

    # ---- methods common in behaviour but dialect-specific in SQL ----

    def check_binary(self) -> None:
        raise NotImplementedError

    def execute(self, sql: str) -> None:
        raise NotImplementedError

    def query(self, sql: str) -> list[list[str]]:
        raise NotImplementedError

    def run_sql_file(self, path: Path) -> None:
        raise NotImplementedError

    def list_tables(self) -> list[str]:
        raise NotImplementedError

    def table_column_types(self, table: str) -> dict[str, str]:
        raise NotImplementedError

    def table_row_count(self, table: str) -> int:
        q = self.quote_ident(table)
        rows = self.query(f"SELECT COUNT(*) FROM {q}")
        if not rows:
            raise MigrationError(f"Failed to count rows in {table}")
        return int(rows[0][0])

    def auto_increment_columns(self, table: str) -> list[str]:
        """Return column names that carry an auto-assigned identity
        (PG: nextval default; MySQL: AUTO_INCREMENT)."""
        raise NotImplementedError

    # ---- script-building primitives ----

    def quote_ident(self, ident: str) -> str:
        raise NotImplementedError

    def format_literal(self, value: object, col_type: str | None) -> str:
        raise NotImplementedError

    def render_load_header(self) -> str:
        """SQL to emit before TRUNCATE + INSERTs."""
        raise NotImplementedError

    def render_load_footer(self) -> str:
        """SQL to emit after TRUNCATE + INSERTs."""
        raise NotImplementedError

    def render_truncate(self, tables: list[str]) -> str:
        raise NotImplementedError

    def render_sequence_resets(
        self, conn: sqlite3.Connection, table: str, cols: list[str]
    ) -> str:
        """SQL to reset the table's auto-increment to MAX(col) + 1."""
        raise NotImplementedError


# ---------------------------------------------------------------------------
# PostgreSQL
# ---------------------------------------------------------------------------


class PsqlClient(TargetClient):
    cli_bin = "psql"

    def __init__(
        self, dsn: TargetDsn, container_exec: ContainerExec | None = None
    ) -> None:
        super().__init__(dsn, container_exec)
        self._cli_args = [
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
        self.env = {"PGPASSWORD": dsn.password}

    def check_binary(self) -> None:
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
                    f"psql not available in container "
                    f"'{self.container_exec.container}': {err}"
                )
            return
        if shutil.which("psql") is None:
            raise MigrationError("psql not found in PATH")

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
            self._run(self.base_args + ["-q"], stdin_path=path)
        else:
            self._run(self.base_args + ["-q", "-f", str(path)])

    def list_tables(self) -> list[str]:
        rows = self.query(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = 'public' AND table_type = 'BASE TABLE' "
            "ORDER BY table_name"
        )
        return [r[0] for r in rows]

    def table_column_types(self, table: str) -> dict[str, str]:
        safe = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name, data_type FROM information_schema.columns "
            f"WHERE table_schema = 'public' AND table_name = '{safe}' "
            "ORDER BY ordinal_position"
        )
        return {r[0]: r[1] for r in rows}

    def auto_increment_columns(self, table: str) -> list[str]:
        safe = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name FROM information_schema.columns "
            f"WHERE table_schema = 'public' AND table_name = '{safe}' "
            "AND column_default LIKE 'nextval%' "
            "ORDER BY ordinal_position"
        )
        return [r[0] for r in rows]

    def quote_ident(self, ident: str) -> str:
        return '"' + ident.replace('"', '""') + '"'

    def format_literal(self, value: object, col_type: str | None) -> str:
        if value is None:
            return "NULL"
        if col_type == "boolean":
            if isinstance(value, bool):
                return "TRUE" if value else "FALSE"
            if isinstance(value, int):
                return "TRUE" if value != 0 else "FALSE"
            if isinstance(value, str):
                v = value.strip().lower()
                if v in ("1", "t", "true", "yes", "y"):
                    return "TRUE"
                if v in ("0", "f", "false", "no", "n", ""):
                    return "FALSE"
                raise MigrationError(f"Cannot convert string {value!r} to boolean")
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

    def render_load_header(self) -> str:
        return "BEGIN;\n"

    def render_load_footer(self) -> str:
        return "COMMIT;\n"

    def render_truncate(self, tables: list[str]) -> str:
        if not tables:
            return ""
        quoted = ", ".join(self.quote_ident(t) for t in tables)
        return f"TRUNCATE TABLE {quoted} RESTART IDENTITY CASCADE;\n"

    def render_sequence_resets(
        self, conn: sqlite3.Connection, table: str, cols: list[str]
    ) -> str:
        if not cols:
            return ""
        out: list[str] = []
        safe_t = table.replace("'", "''")
        for col in cols:
            safe_c = col.replace("'", "''")
            q_col = self.quote_ident(col)
            q_tab = self.quote_ident(table)
            out.append(
                f"SELECT setval(pg_get_serial_sequence('{safe_t}', '{safe_c}'), "
                f"COALESCE((SELECT MAX({q_col}) FROM {q_tab}), 0) + 1, false);\n"
            )
        return "".join(out)


# ---------------------------------------------------------------------------
# MySQL / MariaDB
# ---------------------------------------------------------------------------


class MysqlClient(TargetClient):
    cli_bin = "mysql"

    def __init__(
        self, dsn: TargetDsn, container_exec: ContainerExec | None = None
    ) -> None:
        super().__init__(dsn, container_exec)
        self._rebuild_cli_args()
        self.env = {"MYSQL_PWD": dsn.password} if dsn.password else {}

    def _rebuild_cli_args(self) -> None:
        self._cli_args = [
            self.cli_bin,
            "--batch",
            "--raw",
            "--skip-column-names",
            "-h",
            self.dsn.host,
            "-P",
            str(self.dsn.port),
            "-u",
            self.dsn.user,
            self.dsn.database,
        ]

    def check_binary(self) -> None:
        if self.container_exec:
            if shutil.which(self.container_exec.runtime) is None:
                raise MigrationError(
                    f"{self.container_exec.runtime} not found in PATH"
                )
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
                    if candidate != self.cli_bin:
                        self.cli_bin = candidate
                        self._rebuild_cli_args()
                    return
            raise MigrationError(
                "neither mysql nor mariadb client available in container "
                f"'{self.container_exec.container}'"
            )
        for candidate in ("mysql", "mariadb"):
            if shutil.which(candidate) is not None:
                if candidate != self.cli_bin:
                    self.cli_bin = candidate
                    self._rebuild_cli_args()
                return
        raise MigrationError("neither mysql nor mariadb client found in PATH")

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
        self._run(self.base_args, stdin_path=path)

    def list_tables(self) -> list[str]:
        rows = self.query(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' "
            "ORDER BY table_name"
        )
        return [r[0] for r in rows]

    def table_column_types(self, table: str) -> dict[str, str]:
        safe = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name, data_type FROM information_schema.columns "
            f"WHERE table_schema = DATABASE() AND table_name = '{safe}' "
            "ORDER BY ordinal_position"
        )
        return {r[0]: r[1] for r in rows}

    def auto_increment_columns(self, table: str) -> list[str]:
        safe = table.replace("'", "''")
        rows = self.query(
            "SELECT column_name FROM information_schema.columns "
            f"WHERE table_schema = DATABASE() AND table_name = '{safe}' "
            "AND extra LIKE '%auto_increment%' "
            "ORDER BY ordinal_position"
        )
        return [r[0] for r in rows]

    def quote_ident(self, ident: str) -> str:
        return "`" + ident.replace("`", "``") + "`"

    def format_literal(self, value: object, col_type: str | None) -> str:
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

    def render_load_header(self) -> str:
        # TRUNCATE auto-commits in MySQL, so we cannot wrap the whole sequence
        # in a transaction. SET FOREIGN_KEY_CHECKS=0 is session-local and
        # requires no special privilege; it persists until we re-enable below.
        return "SET FOREIGN_KEY_CHECKS = 0;\n"

    def render_load_footer(self) -> str:
        return "SET FOREIGN_KEY_CHECKS = 1;\n"

    def render_truncate(self, tables: list[str]) -> str:
        # MySQL TRUNCATE only takes a single table per statement.
        return "".join(
            f"TRUNCATE TABLE {self.quote_ident(t)};\n" for t in tables
        )

    def render_sequence_resets(
        self, conn: sqlite3.Connection, table: str, cols: list[str]
    ) -> str:
        if not cols:
            return ""
        out: list[str] = []
        q_tab = self.quote_ident(table)
        for col in cols:
            # Preload the next auto-increment value from the SQLite source.
            row = conn.execute(f'SELECT MAX("{col}") FROM "{table}"').fetchone()
            next_val = int(row[0]) + 1 if row and row[0] is not None else 1
            out.append(f"ALTER TABLE {q_tab} AUTO_INCREMENT = {next_val};\n")
        return "".join(out)


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------


def make_target_client(
    dsn: TargetDsn, container_exec: ContainerExec | None
) -> TargetClient:
    if dsn.backend == "postgres":
        return PsqlClient(dsn, container_exec)
    if dsn.backend == "mysql":
        return MysqlClient(dsn, container_exec)
    raise MigrationError(f"Unsupported backend: {dsn.backend}")


def sqlite_tables(conn: sqlite3.Connection) -> list[str]:
    rows = conn.execute(
        "SELECT name FROM sqlite_master "
        "WHERE type = 'table' AND name NOT LIKE 'sqlite_%' "
        "ORDER BY name"
    ).fetchall()
    return [r[0] for r in rows]


def sqlite_columns(conn: sqlite3.Connection, table: str) -> list[str]:
    rows = conn.execute(f'PRAGMA table_info("{table}")').fetchall()
    return [r[1] for r in rows]


def sqlite_foreign_keys(conn: sqlite3.Connection, table: str) -> list[str]:
    rows = conn.execute(f'PRAGMA foreign_key_list("{table}")').fetchall()
    return [r[2] for r in rows]


def sqlite_self_ref_columns(
    conn: sqlite3.Connection, table: str
) -> list[tuple[str, str]]:
    """Return (from_col, to_col) pairs for FK constraints that reference the
    same table."""
    rows = conn.execute(f'PRAGMA foreign_key_list("{table}")').fetchall()
    return [(r[3], r[4]) for r in rows if r[2] == table]


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
            "Dependency cycles (likely self-references); appending in name order: %s",
            ", ".join(remaining),
        )
        ordered.extend(remaining)
    return ordered


def topologically_sort_rows(
    rows: list[tuple[object, ...]],
    columns: list[str],
    self_refs: list[tuple[str, str]],
) -> list[tuple[object, ...]]:
    """Order rows so every self-FK target appears earlier.

    Handles nullable and NOT NULL self-FKs. Raises MigrationError on cycles
    or dangling references (neither should occur in a consistent source)."""
    col_idx = {c: i for i, c in enumerate(columns)}
    for from_c, to_c in self_refs:
        if from_c not in col_idx or to_c not in col_idx:
            raise MigrationError(
                f"Self-ref ({from_c} -> {to_c}) references unknown column"
            )

    inserted: set[tuple[str, object]] = set()
    ordered: list[tuple[object, ...]] = []
    remaining = list(rows)

    while remaining:
        progressed: list[tuple[object, ...]] = []
        for row in remaining:
            ready = True
            for from_c, to_c in self_refs:
                val = row[col_idx[from_c]]
                if val is None:
                    continue
                if (to_c, val) not in inserted:
                    ready = False
                    break
            if ready:
                ordered.append(row)
                for _, to_c in self_refs:
                    inserted.add((to_c, row[col_idx[to_c]]))
                progressed.append(row)
        if not progressed:
            raise MigrationError(
                "Cannot topologically sort rows for self-referential table: "
                "cycle or dangling reference detected"
            )
        for r in progressed:
            remaining.remove(r)

    return ordered


def count_rows(conn: sqlite3.Connection, table: str) -> int:
    row = conn.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()
    return int(row[0]) if row else 0


def iter_rows(
    conn: sqlite3.Connection, table: str, columns: list[str]
) -> Iterable[list[tuple[object, ...]]]:
    q_cols = ", ".join(f'"{c}"' for c in columns)
    cur = conn.execute(f'SELECT {q_cols} FROM "{table}"')
    while True:
        chunk = cur.fetchmany(FETCH_BATCH_SIZE)
        if not chunk:
            break
        yield chunk


def write_insert_statements(
    client: TargetClient,
    out: TextIO,
    table: str,
    columns: list[str],
    col_types: list[str | None],
    rows: Iterable[tuple[object, ...]],
) -> int:
    q_table = client.quote_ident(table)
    q_cols = ", ".join(client.quote_ident(c) for c in columns)
    prefix = f"INSERT INTO {q_table} ({q_cols}) VALUES "
    written = 0
    current: list[str] = []
    size = len(prefix) + 1
    for row in rows:
        row_sql = (
            "("
            + ", ".join(
                client.format_literal(v, t) for v, t in zip(row, col_types)
            )
            + ")"
        )
        row_size = len(row_sql) + 2
        if current and (size + row_size > MAX_STATEMENT_BYTES):
            out.write(prefix)
            out.write(", ".join(current))
            out.write(";\n")
            current = [row_sql]
            size = len(prefix) + len(row_sql) + 1
        else:
            current.append(row_sql)
            size += row_size
        written += 1
    if current:
        out.write(prefix)
        out.write(", ".join(current))
        out.write(";\n")
    return written


def confirm(dsn: TargetDsn, container_exec: ContainerExec | None) -> None:
    if container_exec:
        target = (
            f"  {dsn.backend}://{dsn.user}@{dsn.host}:{dsn.port}/{dsn.database}\n"
            f"  (via {container_exec.runtime} exec -i {container_exec.container})\n"
        )
    else:
        target = f"  {dsn.backend}://{dsn.user}@{dsn.host}:{dsn.port}/{dsn.database}\n"
    if dsn.backend == "postgres":
        txn_note = (
            "All steps run inside one BEGIN/COMMIT; a failure rolls back and\n"
            "leaves the target unchanged.\n"
        )
    else:
        txn_note = (
            "MySQL TRUNCATE auto-commits — steps 1–3 are NOT atomic. If an\n"
            "error occurs mid-load, the target will be in a partial state;\n"
            "re-run to complete the migration.\n"
        )
    msg = (
        "This will modify the target database destructively:\n"
        f"{target}"
        "Actions:\n"
        "  1. TRUNCATE every data table in the target (except _sqlx_migrations).\n"
        "  2. Copy all rows from the SQLite source.\n"
        "  3. Reset auto-increment sequences to MAX(id) + 1.\n"
        f"{txn_note}"
    )
    if not sys.stdin.isatty():
        raise MigrationError(
            "Non-interactive stdin; refusing to proceed without confirmation."
        )
    print(msg, file=sys.stderr, end="")
    try:
        resp = input("Proceed? [y/N]: ").strip().lower()
    except EOFError:
        resp = ""
    if resp not in ("y", "yes"):
        raise MigrationError("Aborted by user")


def build_load_script(
    client: TargetClient,
    conn: sqlite3.Connection,
    ordered_tables: list[str],
    columns_map: dict[str, list[str]],
    col_types_map: dict[str, list[str | None]],
    auto_inc_map: dict[str, list[str]],
    self_ref_map: dict[str, list[tuple[str, str]]],
    out: TextIO,
) -> int:
    """Write the complete load script to `out` and return rows emitted."""
    out.write(client.render_load_header())
    out.write(client.render_truncate(ordered_tables))

    total = 0
    for idx, table in enumerate(ordered_tables, start=1):
        cols = columns_map[table]
        types = col_types_map[table]
        self_refs = self_ref_map[table]
        table_total = count_rows(conn, table)
        LOG.info("[%d/%d] %s: %d rows", idx, len(ordered_tables), table, table_total)
        if table_total == 0:
            continue

        if self_refs:
            LOG.info("%s has self-reference(s); sorting rows by dependency", table)
            q_cols = ", ".join(f'"{c}"' for c in cols)
            all_rows = conn.execute(f'SELECT {q_cols} FROM "{table}"').fetchall()
            sorted_rows = topologically_sort_rows(all_rows, cols, self_refs)
            total += write_insert_statements(
                client, out, table, cols, types, sorted_rows
            )
        else:
            for batch in iter_rows(conn, table, cols):
                total += write_insert_statements(
                    client, out, table, cols, types, batch
                )

    for table in ordered_tables:
        out.write(
            client.render_sequence_resets(conn, table, auto_inc_map.get(table, []))
        )

    out.write(client.render_load_footer())
    return total


def migrate(
    sqlite_path: Path, dsn: TargetDsn, container_exec: ContainerExec | None
) -> None:
    if not sqlite_path.exists():
        raise MigrationError(f"SQLite DB not found: {sqlite_path}")

    client = make_target_client(dsn, container_exec)
    client.check_binary()
    confirm(dsn, container_exec)

    LOG.info("Opening SQLite DB: %s", sqlite_path)
    conn = sqlite3.connect(f"file:{sqlite_path}?mode=ro", uri=True)
    conn.row_factory = None

    try:
        source_tables = [t for t in sqlite_tables(conn) if t not in EXCLUDED_TABLES]
        if not source_tables:
            raise MigrationError("No source tables found in SQLite database")

        target_tables = set(client.list_tables())
        missing_tables = [t for t in source_tables if t not in target_tables]
        if missing_tables:
            raise MigrationError(
                "Target DB is missing required tables: "
                + ", ".join(sorted(missing_tables))
                + ". Run 'ropds --init-db' against the target first."
            )
        if "_sqlx_migrations" not in target_tables:
            raise MigrationError(
                "Target DB has no _sqlx_migrations table. "
                "Run 'ropds --init-db' against the target first."
            )

        columns_map: dict[str, list[str]] = {}
        col_types_map: dict[str, list[str | None]] = {}
        auto_inc_map: dict[str, list[str]] = {}
        self_ref_map: dict[str, list[tuple[str, str]]] = {}
        for table in source_tables:
            src_cols = sqlite_columns(conn, table)
            dst_types = client.table_column_types(table)
            missing_cols = [c for c in src_cols if c not in dst_types]
            if missing_cols:
                raise MigrationError(
                    f"Table '{table}' schema mismatch; target is missing columns: "
                    + ", ".join(missing_cols)
                )
            columns_map[table] = src_cols
            col_types_map[table] = [dst_types.get(c) for c in src_cols]
            auto_inc_map[table] = client.auto_increment_columns(table)
            self_ref_map[table] = sqlite_self_ref_columns(conn, table)

        ordered = topological_order(conn, source_tables)
        LOG.info("Will migrate %d tables", len(ordered))

        LOG.info("Precheck: verifying every target data table is empty")
        populated: list[str] = []
        for table in ordered:
            dst_count = client.table_row_count(table)
            if dst_count != 0:
                populated.append(f"{table}: {dst_count} rows")
        if populated:
            raise MigrationError(
                "Target database is not empty — the following data tables "
                "already contain rows:\n  "
                + "\n  ".join(populated)
                + "\nExpected state after 'ropds --init-db': every data table "
                "has 0 rows (init-db clears seed data as part of migration prep). "
                "Refusing to proceed."
            )

        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            suffix=f".{dsn.backend}.sql",
            prefix="ropds_migrate_",
            delete=False,
        ) as tmp:
            script_path = Path(tmp.name)

        started = time.monotonic()
        try:
            with script_path.open("w", encoding="utf-8") as out:
                total_rows = build_load_script(
                    client,
                    conn,
                    ordered,
                    columns_map,
                    col_types_map,
                    auto_inc_map,
                    self_ref_map,
                    out,
                )
            LOG.info(
                "Built load script (%d bytes); executing in a single %s session",
                script_path.stat().st_size,
                client.cli_bin,
            )
            client.run_sql_file(script_path)
        finally:
            script_path.unlink(missing_ok=True)

        LOG.info("Verifying row counts")
        for table in ordered:
            src_count = count_rows(conn, table)
            dst_count = client.table_row_count(table)
            if src_count != dst_count:
                raise MigrationError(
                    f"Row count mismatch for '{table}': source={src_count}, target={dst_count}"
                )
            LOG.info("Verified %s: %d rows", table, dst_count)

        elapsed = time.monotonic() - started
        LOG.info(
            "Migration completed: %d tables, %d rows, %.1f seconds",
            len(ordered),
            total_rows,
            elapsed,
        )
    finally:
        conn.close()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Copy ROPDS data from SQLite to PostgreSQL or MySQL/MariaDB.",
        epilog="The target schema must already exist. "
        "Run 'ropds --init-db' against the target before using this script.",
    )
    parser.add_argument("sqlite_db", help="Path to source SQLite database file")
    parser.add_argument(
        "target_url",
        help=(
            "Target DB URL: postgres://user:pass@host:port/db "
            "or mysql://user:pass@host:port/db"
        ),
    )
    parser.add_argument(
        "--db-container",
        help=(
            "Run the target DB CLI (psql/mysql) inside this running container "
            "using '<runtime> exec -i'. Useful when the host has no client installed."
        ),
    )
    parser.add_argument(
        "--container-runtime",
        default="docker",
        choices=["docker", "podman"],
        help="Container runtime for --db-container (default: docker)",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s"
    )

    try:
        sqlite_path = Path(args.sqlite_db).expanduser()
        dsn = parse_target_dsn(args.target_url)
        container_exec: ContainerExec | None = None
        if args.db_container:
            container_exec = ContainerExec(
                runtime=args.container_runtime, container=args.db_container
            )
        migrate(sqlite_path, dsn, container_exec)
        return 0
    except KeyboardInterrupt:
        LOG.error("Interrupted by user")
        return 130
    except MigrationError as e:
        LOG.error("Migration failed: %s", e)
        return 1
    except Exception:  # pragma: no cover
        LOG.exception("Unexpected failure")
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
