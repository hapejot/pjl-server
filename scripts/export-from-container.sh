#!/usr/bin/env bash
# Fuehrt odata-export in einem laufenden Docker-Container aus und holt das Ergebnis heraus.
#
# Ablauf:
#   1. odata-export wird im rust:1-bookworm-Container gebaut (glibc passend zu Debian
#      bookworm, dem Basis-Image der Runtime). Das Binary liegt danach unter target/container/.
#   2. Das Binary wird per `docker cp` in den Zielcontainer kopiert.
#   3. Im Zielcontainer laeuft der Export gegen die Runtime (Standard: http://localhost:8080/Service,
#      so lauscht die Runtime im Image, Variable PORT).
#   4. Das Ergebnisverzeichnis wird per `docker cp` lokal abgelegt, Temp-Dateien im Container
#      werden entfernt.
#
# Verwendung:
#   scripts/export-from-container.sh CONTAINER [--out DIR] [--base-url URL] [--sets A,B] [--rebuild]
#
# Beispiel:
#   scripts/export-from-container.sh poc-crm-runtime-1 --out ./export-poc-crm

set -euo pipefail

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

[[ $# -ge 1 ]] || usage 1
case "$1" in -h|--help) usage 0 ;; esac
CONTAINER="$1"
shift

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BASE_URL="http://localhost:8080/Service"
OUT=""
SETS=""
REBUILD=0
BUILD_IMAGE="${BUILD_IMAGE:-rust:1-bookworm}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --base-url) BASE_URL="$2"; shift 2 ;;
    --sets) SETS="$2"; shift 2 ;;
    --rebuild) REBUILD=1; shift ;;
    -h|--help) usage 0 ;;
    *) echo "unbekannte Option: $1" >&2; usage 1 ;;
  esac
done

command -v docker >/dev/null || { echo "docker nicht gefunden" >&2; exit 1; }
if [[ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null || true)" != "true" ]]; then
  echo "Container '$CONTAINER' laeuft nicht" >&2
  exit 1
fi

BIN="$REPO/target/container/odata-export"
if [[ $REBUILD -eq 1 || ! -x "$BIN" ]]; then
  echo "Baue odata-export und odata-import im Container ($BUILD_IMAGE) ..."
  mkdir -p "$REPO/target/container"
  docker run --rm \
    -v "$REPO:/src" \
    -w /src \
    -e CARGO_TARGET_DIR=/src/target/container/build \
    -v pjl-export-cargo-registry:/usr/local/cargo/registry \
    "$BUILD_IMAGE" \
    cargo build --release -p odata-export
  cp "$REPO/target/container/build/release/odata-export" "$BIN"
  cp "$REPO/target/container/build/release/odata-import" "$REPO/target/container/odata-import"
  chmod 755 "$BIN" "$REPO/target/container/odata-import"
fi

# Eigenes Arbeitsverzeichnis: /tmp ist sticky, Dateien aus `docker cp` gehoeren der
# Host-UID und koennen vom Container-User dort nicht geloescht werden. In einem
# Verzeichnis, das der Container-User selbst anlegt, geht das Aufraeumen.
REMOTE_DIR="/tmp/pjl-export"
REMOTE_BIN="$REMOTE_DIR/odata-export"
REMOTE_OUT="$REMOTE_DIR/out"
OUT="${OUT:-$REPO/export-$CONTAINER-$(date +%Y%m%d-%H%M%S)}"

cleanup_remote() {
  docker exec "$CONTAINER" rm -rf "$REMOTE_DIR" >/dev/null 2>&1 || true
}
trap cleanup_remote EXIT

docker exec "$CONTAINER" rm -rf "$REMOTE_DIR"
docker exec "$CONTAINER" mkdir -p "$REMOTE_DIR"
echo "Kopiere Export-Programm nach $CONTAINER:$REMOTE_BIN"
docker cp "$BIN" "$CONTAINER:$REMOTE_BIN"

ARGS=(--base-url "$BASE_URL" --out "$REMOTE_OUT")
[[ -n "$SETS" ]] && ARGS+=(--sets "$SETS")
echo "Export im Container gegen $BASE_URL ..."
docker exec "$CONTAINER" "$REMOTE_BIN" "${ARGS[@]}"

echo "Hole Ergebnis nach $OUT"
rm -rf "$OUT"
mkdir -p "$(dirname "$OUT")"
docker cp "$CONTAINER:$REMOTE_OUT" "$OUT"
echo "Fertig: $OUT ($(ls "$OUT" | wc -l) Dateien)"
