#!/bin/sh
set -eu

log() {
    printf '%s %s\n' "[wait-for-db]" "$*"
}

if [ "$#" -gt 0 ]; then
    DB_URL="$1"
else
    DB_URL="${ROPDS_DATABASE_URL:-}"
fi
TIMEOUT="${2:-${ROPDS_DB_WAIT_TIMEOUT:-60}}"
INTERVAL="${ROPDS_DB_WAIT_INTERVAL:-2}"
DB_HOST_OVERRIDE="${ROPDS_DB_HOST:-}"
DB_PORT_OVERRIDE="${ROPDS_DB_PORT:-}"

case "$DB_URL" in
    postgres://*|postgresql://*)
        DEFAULT_PORT=5432
        ;;
    mysql://*)
        DEFAULT_PORT=3306
        ;;
    *)
        DEFAULT_PORT=""
        ;;
esac

host=""
port=""

if [ -n "$DB_HOST_OVERRIDE" ]; then
    host="$DB_HOST_OVERRIDE"
    if [ -n "$DB_PORT_OVERRIDE" ]; then
        port="$DB_PORT_OVERRIDE"
    elif [ -n "$DEFAULT_PORT" ]; then
        port="$DEFAULT_PORT"
    else
        log "ERROR: ROPDS_DB_HOST set but neither ROPDS_DB_PORT nor known DB URL scheme provided"
        exit 2
    fi
else
    if [ -z "$DB_URL" ]; then
        log "INFO: empty DB URL and no ROPDS_DB_HOST override; skipping wait"
        exit 0
    fi
    if [ -z "$DEFAULT_PORT" ]; then
        log "INFO: DB URL is not postgres/mysql, skipping wait: $DB_URL"
        exit 0
    fi

    rest="${DB_URL#*://}"
    authority="${rest%%/*}"

    case "$authority" in
        *@*) hostport="${authority##*@}" ;;
        *) hostport="$authority" ;;
    esac

    if [ -z "$hostport" ]; then
        log "ERROR: could not parse host from URL: $DB_URL"
        exit 1
    fi

    case "$hostport" in
        \[*\]*)
            host="${hostport#\[}"
            host="${host%%\]*}"
            after_bracket="${hostport#*\]}"
            case "$after_bracket" in
                :*) port="${after_bracket#:}" ;;
                *) port="$DEFAULT_PORT" ;;
            esac
            ;;
        *:*)
            host="${hostport%%:*}"
            port="${hostport##*:}"
            ;;
        *)
            host="$hostport"
            port="$DEFAULT_PORT"
            ;;
    esac
fi

if [ -z "$host" ]; then
    log "ERROR: parsed empty host from URL: $DB_URL"
    exit 1
fi

if [ -z "$port" ]; then
    port="$DEFAULT_PORT"
fi

if ! [ "$TIMEOUT" -ge 1 ] 2>/dev/null; then
    log "ERROR: invalid timeout '$TIMEOUT'"
    exit 2
fi

start_ts="$(date +%s)"

log "Waiting for DB at $host:$port (timeout ${TIMEOUT}s)"
while ! nc -z "$host" "$port" >/dev/null 2>&1; do
    now_ts="$(date +%s)"
    elapsed="$((now_ts - start_ts))"
    if [ "$elapsed" -ge "$TIMEOUT" ]; then
        log "ERROR: timeout while waiting for $host:$port"
        exit 1
    fi
    sleep "$INTERVAL"
done

log "DB is reachable at $host:$port"
