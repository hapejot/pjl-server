#!/usr/bin/env bash
# Spielt einen Export (aus export-from-container.sh oder odata-export) in einen laufenden
# Docker-Container zurueck: odata-import laeuft im Container gegen dessen Runtime.
#
# Ablauf:
#   1. odata-import wird wie bei export-from-container.sh im rust:1-bookworm-Container gebaut
#      (target/container/odata-import).
#   2. Binary und Exportverzeichnis werden per `docker cp` in den Container kopiert.
#   3. odata-import laeuft im Container gegen die Runtime (Standard: http://localhost:8080/Service).
#   4. Temp-Dateien im Container werden entfernt.
#
# Sicherheit: Ohne --apply ist es ein Probelauf. Er prueft Schluessel und Fremdschluessel-
# Uebersetzung und schreibt nichts. Erst --apply schreibt in die Runtime des Containers.
#
# Verwendung:
#   scripts/import-into-container.sh CONTAINER --from DIR [--apply] [--base-url URL] [--rebuild]

set -euo pipefail

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

[[ $# -ge 1 ]] || usage 1
case "$1" in -h|--help) usage 0 ;; esac
CONTAINER="$1"
shift

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BASE_URL="http://localhost:8080/Service"
SOURCE=""
APPLY=0
REBUILD=0
BUILD_IMAGE="${BUILD_IMAGE:-rust:1-bookworm}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from) SOURCE="$2"; shift 2 ;;
    --base-url) BASE_URL="$2"; shift 2 ;;
    --apply) APPLY=1; shift ;;
    --rebuild) REBUILD=1; shift ;;
    -h|--help) usage 0 ;;
    *) echo "unbekannte Option: $1" >&2; usage 1 ;;
  esac
done

[[ -n "$SOURCE" ]] || { echo "--from DIR fehlt" >&2; exit 1; }
[[ -f "$SOURCE/manifest.json" ]] || { echo "kein Export in $SOURCE (manifest.json fehlt)" >&2; exit 1; }
SOURCE="$(cd "$SOURCE" && pwd)"

command -v docker >/dev/null || { echo "docker nicht gefunden" >&2; exit 1; }
if [[ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null || true)" != "true" ]]; then
  echo "Container '$CONTAINER' laeuft nicht" >&2
  exit 1
fi

BIN="$REPO/target/container/odata-import"
if [[ $REBUILD -eq 1 || ! -x "$BIN" ]]; then
  echo "Baue odata-import im Container ($BUILD_IMAGE) ..."
  mkdir -p "$REPO/target/container"
  docker run --rm \
    -v "$REPO:/src" \
    -w /src \
    -e CARGO_TARGET_DIR=/src/target/container/build \
    -v pjl-export-cargo-registry:/usr/local/cargo/registry \
    "$BUILD_IMAGE" \
    cargo build --release -p odata-export
  cp "$REPO/target/container/build/release/odata-export" "$REPO/target/container/odata-export"
  cp "$REPO/target/container/build/release/odata-import" "$BIN"
  chmod 755 "$REPO/target/container/odata-export" "$BIN"
fi

# Eigenes Arbeitsverzeichnis (siehe export-from-container.sh: /tmp ist sticky).
REMOTE_DIR="/tmp/pjl-import"
REMOTE_BIN="$REMOTE_DIR/odata-import"
REMOTE_DATA="$REMOTE_DIR/data"

# Aufraeumen als root: `docker cp` legt Dateien mit der Host-UID an, der Container-User
# kann sie dann nicht loeschen. Es wird nur das eigene Arbeitsverzeichnis entfernt.
cleanup_remote() {
  docker exec -u 0 "$CONTAINER" rm -rf "$REMOTE_DIR" >/dev/null 2>&1 || true
}
trap cleanup_remote EXIT

docker exec -u 0 "$CONTAINER" rm -rf "$REMOTE_DIR"
docker exec "$CONTAINER" mkdir -p "$REMOTE_DIR"
echo "Kopiere Import-Programm und Export nach $CONTAINER:$REMOTE_DIR"
docker cp "$BIN" "$CONTAINER:$REMOTE_BIN"
docker cp "$SOURCE" "$CONTAINER:$REMOTE_DATA"

ARGS=(--base-url "$BASE_URL" --from "$REMOTE_DATA")
if [[ $APPLY -eq 0 ]]; then
  ARGS+=(--dry-run)
  echo "Probelauf gegen $BASE_URL (ohne --apply wird nichts geschrieben)"
else
  echo "IMPORT gegen $BASE_URL (schreibt in die Runtime des Containers)"
fi
docker exec "$CONTAINER" "$REMOTE_BIN" "${ARGS[@]}"
