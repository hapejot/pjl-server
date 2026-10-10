#!/bin/sh
# Migrationsphase: Katalog lesen, ddic.json schreiben, Daten einmalig importieren.
#
# Idempotent ueber die Marker-Datei im Export-Volume: existiert sie, ist der
# Import bereits gelaufen und der Schritt endet sofort. Mit `--force` wird der
# Store verworfen und neu aufgebaut.
#
# Schlaegt der Import fehl, wird das Datenverzeichnis wieder geleert. So bleibt
# kein halber Store zurueck, der den naechsten Lauf blockiert.
set -eu

DATA="${DATA_DIR:-/app/data}"
EXPORT="${EXPORT_DIR:-/app/export}"
MARKER="$EXPORT/.migrated"
: "${VIDEO_PG_URL:?VIDEO_PG_URL ist nicht gesetzt}"

if [ "${1:-}" = "--force" ]; then
    echo "migrate: --force, vorhandenen Store wird verworfen"
    rm -f "$MARKER"
fi

if [ -f "$MARKER" ]; then
    echo "migrate: bereits erledigt ($(cat "$MARKER")). Zum Wiederholen: --force"
    exit 0
fi

cleanup() {
    rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "migrate: fehlgeschlagen, leere $DATA" >&2
        find "$DATA" -mindepth 1 -delete
    fi
}
trap cleanup EXIT

find "$DATA" -mindepth 1 -delete
echo "migrate: ddic.json erzeugen"
/app/video-ddic --pg "$VIDEO_PG_URL" --out "$EXPORT/ddic.json"
echo "migrate: Daten importieren"
/app/video-import --pg "$VIDEO_PG_URL" --data-dir "$DATA"
date -u +"importiert am %Y-%m-%dT%H:%M:%SZ" > "$MARKER"
echo "migrate: fertig"
