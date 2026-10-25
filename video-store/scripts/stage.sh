#!/usr/bin/env bash
# Migrations-Werkzeug. Quelle wird nur gelesen (pg_dump).
#
# Arbeitet in der Datenbank `video_mig` auf der Instanz `db` (compose.yaml).
# Schemata dort:
#   public  Zielmodell (wird per Load befuellt)
#   src     Quelldaten, aus dem Snapshot per Schema-Umbenennung
#   stage   Zwischenergebnisse, Protokolle (migration/sql/)
# Die Runtime-Datenbank `video` wird von diesem Werkzeug nie veraendert.
#
#   up                Datenbank-Container starten
#   snapshot          pg_dump der Quelle (VIDEO_PG_URL) nach snapshots/
#   restore [datei]   video_mig neu aufbauen, Snapshot als Schema src
#   migrate           migration/sql/*.sql, nur noch nicht angewendete (-> stage)
#   schema            Speicherschema der Runtime aus ddic.json in video_mig
#   load              migration/load/*.sql (stage -> public, nur auf leerem Ziel)
#   validate          tests/sql/*.sql. Jede Pruefung gibt nur bei Fehlern Zeilen aus.
#   rebuild [datei]   restore + migrate + schema + load + validate
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ADMIN_URL="${ADMIN_URL:-postgres://video:video@localhost:5001/postgres}"
MIG_DB="${MIG_DB:-video_mig}"
MIG_URL="${MIG_URL:-postgres://video:video@localhost:5001/$MIG_DB}"
SNAP_DIR="$ROOT/snapshots"
MIG_DIR="$ROOT/migration/sql"
LOAD_DIR="$ROOT/migration/load"
TEST_DIR="$ROOT/tests/sql"

usage() {
    sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'
}

cmd_up() {
    docker compose -f "$ROOT/compose.yaml" up -d --wait db
}

cmd_snapshot() {
    : "${VIDEO_PG_URL:?VIDEO_PG_URL ist nicht gesetzt}"
    mkdir -p "$SNAP_DIR"
    local out="$SNAP_DIR/video-$(date -u +%Y%m%dT%H%M%SZ).dump"
    pg_dump --format=custom --no-owner --no-privileges --file "$out" "$VIDEO_PG_URL"
    echo "snapshot: $out"
}

cmd_restore() {
    cmd_up
    local snap="${1:-}"
    if [ -z "$snap" ]; then
        snap="$(ls -1t "$SNAP_DIR"/*.dump 2>/dev/null | head -n 1 || true)"
    fi
    [ -n "$snap" ] && [ -f "$snap" ] || { echo "restore: kein Snapshot gefunden" >&2; exit 1; }

    echo "restore: $snap -> $MIG_DB"
    psql "$ADMIN_URL" -v ON_ERROR_STOP=1 -q -c "DROP DATABASE IF EXISTS $MIG_DB WITH (FORCE)"
    psql "$ADMIN_URL" -v ON_ERROR_STOP=1 -q -c "CREATE DATABASE $MIG_DB"

    # 1. pgvector in public installieren, damit der Dump die Typen findet.
    # 2. Dump in public einspielen.
    # 3. public nach src umbenennen. Die Tabellen und die Extension wandern mit.
    # 4. Neues, leeres public anlegen und die Extension dorthin zurueckverschieben.
    #    Die Tabellen in src verweisen per OID auf den Typ und bleiben gueltig.
    psql "$MIG_URL" -v ON_ERROR_STOP=1 -q -c "CREATE EXTENSION IF NOT EXISTS vector SCHEMA public"
    pg_restore --no-owner --no-privileges -d "$MIG_URL" "$snap" || \
        echo "restore: pg_restore meldete Warnungen (oben pruefen)" >&2
    psql "$MIG_URL" -v ON_ERROR_STOP=1 -q -c \
        "ALTER SCHEMA public RENAME TO src;
         CREATE SCHEMA public;
         ALTER EXTENSION vector SET SCHEMA public;"
    echo "restore: fertig (Schema src, public ist leer)"
}

cmd_migrate() {
    cmd_up
    psql "$MIG_URL" -v ON_ERROR_STOP=1 -q -c \
        "CREATE SCHEMA IF NOT EXISTS stage;
         CREATE TABLE IF NOT EXISTS stage.schema_migrations (
             name text PRIMARY KEY,
             applied_at timestamptz NOT NULL DEFAULT now());"

    local f name done_flag
    shopt -s nullglob
    local files=("$MIG_DIR"/*.sql)
    shopt -u nullglob
    if [ "${#files[@]}" -eq 0 ]; then
        echo "migrate: keine Skripte in migration/sql/"
        return 0
    fi
    for f in "${files[@]}"; do
        name="$(basename "$f")"
        done_flag="$(psql "$MIG_URL" -At -c \
            "SELECT 1 FROM stage.schema_migrations WHERE name = '$name'")"
        if [ -n "$done_flag" ]; then
            echo "skip   $name"
            continue
        fi
        echo "apply  $name"
        # Jedes Skript in einer Transaktion. Bei Fehler bricht psql ab, dann
        # wird nichts committet und der Eintrag fehlt.
        {
            echo "BEGIN;"
            cat "$f"
            echo ";"
            echo "INSERT INTO stage.schema_migrations (name) VALUES ('$name');"
            echo "COMMIT;"
        } | psql "$MIG_URL" -v ON_ERROR_STOP=1 -q -f -
    done
    echo "migrate: fertig"
}

cmd_schema() {
    cmd_up
    # Speicherschema der Runtime aus ddic.json in video_mig anlegen.
    (cd "$ROOT/.." && cargo run -q -p video-store --bin video-schema -- \
        --ddic "$ROOT/ddic.json" --target-pg "$MIG_URL")
}

cmd_load() {
    cmd_up
    local f fail=0
    shopt -s nullglob
    local files=("$LOAD_DIR"/*.sql)
    shopt -u nullglob
    if [ "${#files[@]}" -eq 0 ]; then
        echo "load: keine Skripte in migration/load/"
        return 0
    fi
    for f in "${files[@]}"; do
        echo "load   $(basename "$f")"
        # Eine Transaktion pro Skript (-1). Die Skripte pruefen selbst, dass
        # das Ziel leer ist, damit ein zweiter Lauf abgewiesen wird.
        psql "$MIG_URL" -v ON_ERROR_STOP=1 -q -1 -f "$f" || fail=1
    done
    return "$fail"
}

cmd_validate() {
    cmd_up
    local f out fail=0
    shopt -s nullglob
    local files=("$TEST_DIR"/*.sql)
    shopt -u nullglob
    if [ "${#files[@]}" -eq 0 ]; then
        echo "validate: keine Pruefungen in tests/sql/"
        return 0
    fi
    for f in "${files[@]}"; do
        out="$(psql "$MIG_URL" -v ON_ERROR_STOP=1 -At -f "$f")"
        if [ -n "$out" ]; then
            echo "FAIL   $(basename "$f")"
            echo "$out"
            fail=1
        else
            echo "ok     $(basename "$f")"
        fi
    done
    return "$fail"
}

main() {
    local cmd="${1:-}"
    shift || true
    case "$cmd" in
        up)       cmd_up ;;
        snapshot) cmd_snapshot ;;
        restore)  cmd_restore "$@" ;;
        migrate)  cmd_migrate ;;
        schema)   cmd_schema ;;
        load)     cmd_load ;;
        validate) cmd_validate ;;
        rebuild)  cmd_restore "$@"; cmd_migrate; cmd_schema; cmd_load; cmd_validate ;;
        ""|-h|--help|help) usage ;;
        *) echo "unbekannter Befehl: $cmd" >&2; usage >&2; exit 2 ;;
    esac
}

main "$@"
