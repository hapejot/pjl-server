#!/bin/sh
# Migrationsphase: Katalog lesen, ddic.json schreiben, Daten einmalig importieren.
#
# Quelle ist die bestehende Video-Datenbank (VIDEO_PG_URL, nur lesend), Ziel
# ist die eigene PostgreSQL-Instanz des Stores (STORE_PG_URL).
#
# Idempotent ueber die Marker-Datei im Export-Volume: existiert sie, ist der
# Import bereits gelaufen und der Schritt endet sofort. Mit `--force` wird der
# Store im Ziel geleert und neu aufgebaut.
#
# Schlaegt der Import fehl, wird das Ziel wieder geleert. So bleibt kein halber
# Store zurueck, der den naechsten Lauf blockiert.
set -eu

EXPORT="${EXPORT_DIR:-/app/export}"
MARKER="$EXPORT/.migrated"
export RUST_LOG=info
: "${VIDEO_PG_URL:?VIDEO_PG_URL ist nicht gesetzt}"
: "${STORE_PG_URL:?STORE_PG_URL ist nicht gesetzt}"

RESET=""
if [ "${1:-}" = "--force" ]; then
    echo "migrate: --force, vorhandener Store wird verworfen"
    rm -f "$MARKER"
    RESET="--reset"
fi

if [ -f "$MARKER" ]; then
    echo "migrate: bereits erledigt ($(cat "$MARKER")). Zum Wiederholen: --force"
    exit 0
fi

cleanup() {
    rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "migrate: fehlgeschlagen, leere das Ziel" >&2
        /app/video-import --clear-only || echo "migrate: Ziel konnte nicht geleert werden" >&2
    fi
}
trap cleanup EXIT

echo "migrate: ddic.json erzeugen"
/app/video-ddic --pg "$VIDEO_PG_URL" --out "$EXPORT/ddic.json"
echo "migrate: Daten importieren"
# $RESET ist bewusst ungequotet: leer entfaellt, sonst ein Argument.
/app/video-import $RESET
date -u +"importiert am %Y-%m-%dT%H:%M:%SZ" > "$MARKER"
echo "migrate: fertig"
