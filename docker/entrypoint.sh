#!/bin/sh
set -eu

log() {
    printf '%s %s\n' "[entrypoint]" "$*"
}

ROPDS_BIN="${ROPDS_BIN:-/app/ropds}"
ROPDS_CONFIG="${ROPDS_CONFIG:-/app/config/config.toml}"
ROPDS_DB_WAIT_TIMEOUT="${ROPDS_DB_WAIT_TIMEOUT:-60}"
ROPDS_ADMIN_INIT_ONCE="${ROPDS_ADMIN_INIT_ONCE:-true}"
ROPDS_ADMIN_MARKER_PATH="${ROPDS_ADMIN_MARKER_PATH:-/library/.ropds_admin_initialized}"

require_readable_file() {
    if [ ! -r "$1" ]; then
        log "ERROR: required file is not readable: $1"
        exit 1
    fi
}

bool_is_true() {
    case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
        1|true|yes|on) return 0 ;;
        *) return 1 ;;
    esac
}

# Read a string/bool scalar from a TOML section without full parser dependency.
toml_value() {
    section="$1"
    key="$2"
    awk -v section="$section" -v key="$key" '
        $0 ~ "^[[:space:]]*\\[" section "\\][[:space:]]*(#.*)?$" { in_section=1; next }
        /^[[:space:]]*\[/ { in_section=0 }
        in_section && $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            line=$0
            sub(/^[^=]*=[[:space:]]*/, "", line)
            sub(/[[:space:]]*#.*/, "", line)
            gsub(/^"/, "", line)
            gsub(/"$/, "", line)
            print line
            exit
        }
    ' "$ROPDS_CONFIG"
}

ensure_rw_dir() {
    dir="$1"
    mkdir -p "$dir"
    probe="$dir/.ropds_write_test.$$"
    : > "$probe" || {
        log "ERROR: directory is not writable: $dir"
        exit 1
    }
    rm -f "$probe"
}

require_readable_file "$ROPDS_BIN"
require_readable_file "$ROPDS_CONFIG"

if [ ! -d /app/static ]; then
    log "ERROR: expected static directory at /app/static"
    exit 1
fi

db_url="$(toml_value database url || true)"
if [ -z "$db_url" ]; then
    log "ERROR: could not read [database].url from $ROPDS_CONFIG"
    exit 1
fi

root_path="$(toml_value library root_path || true)"
covers_path="$(toml_value covers covers_path || true)"
legacy_library_covers_path="$(toml_value library covers_path || true)"
legacy_covers_dir="$(toml_value library covers_dir || true)"
upload_path="$(toml_value upload upload_path || true)"
allow_upload="$(toml_value upload allow_upload || true)"

if [ -z "$root_path" ]; then
    root_path="/library"
fi
if [ -z "$covers_path" ]; then
    covers_path="$legacy_library_covers_path"
fi
if [ -z "$covers_path" ]; then
    covers_path="$legacy_covers_dir"
fi
if [ -z "$covers_path" ]; then
    covers_path="$root_path/covers"
fi

ensure_rw_dir "$root_path"
ensure_rw_dir "$covers_path"

if bool_is_true "${allow_upload:-false}"; then
    if [ -z "$upload_path" ]; then
        upload_path="$root_path/uploads"
    fi
    ensure_rw_dir "$upload_path"
fi

case "$db_url" in
    postgres://*|postgresql://*|mysql://*)
        /usr/local/bin/wait-for-db.sh "$db_url" "$ROPDS_DB_WAIT_TIMEOUT"
        ;;
    *)
        log "INFO: skipping DB wait for URL: $db_url"
        ;;
esac

if [ -n "${ROPDS_ADMIN_PASSWORD:-}" ]; then
    if bool_is_true "$ROPDS_ADMIN_INIT_ONCE"; then
        if [ -f "$ROPDS_ADMIN_MARKER_PATH" ]; then
            log "Admin init marker exists, skipping admin initialization"
        else
            log "Initializing admin user (one-time mode)"
            "$ROPDS_BIN" --config "$ROPDS_CONFIG" --set-admin "$ROPDS_ADMIN_PASSWORD"
            marker_dir="$(dirname "$ROPDS_ADMIN_MARKER_PATH")"
            mkdir -p "$marker_dir"
            touch "$ROPDS_ADMIN_MARKER_PATH"
        fi
    else
        log "Initializing admin user (always mode)"
        "$ROPDS_BIN" --config "$ROPDS_CONFIG" --set-admin "$ROPDS_ADMIN_PASSWORD"
    fi
else
    log "WARN: ROPDS_ADMIN_PASSWORD is not set, admin user auto-init skipped"
fi

log "Starting ropds"
exec "$ROPDS_BIN" --config "$ROPDS_CONFIG"
