# Variante des generischen 2-Stage-Dockerfiles für Cargo-WORKSPACES
# (Top-Level Cargo.toml mit mehreren Member-Crates), bei denen am Ende genau
# EIN Binary aus einem der Crates laufen soll, aber der ganze Workspace zum
# Bauen gebraucht wird (z.B. wegen path-Dependencies zwischen den Crates).
#
# Unterschied zur Single-Crate-Variante: Statt des Dummy-main.rs-Caching-
# Tricks werden hier BuildKit-Cache-Mounts für Cargo-Registry und target/
# genutzt - das cached Dependencies genauso effektiv, ganz ohne Stub-Dateien
# pro Crate. Erfordert BuildKit (bei aktuellem Docker Standard).
#
# Build:
#   docker build --build-arg BIN_NAME=mein-server -t mein-server .
# Falls der Binary-Name in mehreren Crates vorkommt oder Cargo das Crate
# nicht eindeutig zuordnen kann, zusätzlich das Package angeben:
#   docker build --build-arg BIN_NAME=mein-server \
#                --build-arg CARGO_BUILD_ARGS="-p mein-crate" \
#                -t mein-server .
# Run:
#   docker run --rm -p 8080:8080 -e PORT=8080 mein-server

########################################
# Stage 1: build
########################################
FROM rust:1-bookworm AS build
ARG BIN_NAME
ARG CARGO_BUILD_ARGS=""
WORKDIR /usr/src/app

# Kompletten Workspace in einem Rutsch kopieren: Top-Level Cargo.toml,
# Cargo.lock und alle Member-Crates. Bei einem Workspace lässt sich das
# Dependency-Layer-Caching per Dummy-Datei kaum sauber pro Crate nachbilden -
# daher hier bewusst alles kopiert und stattdessen über Cache-Mounts unten
# gecacht.
COPY . .

# --mount=type=cache sorgt dafür, dass heruntergeladene Cargo-Registry-Daten
# und bereits kompilierte Dependencies zwischen Builds erhalten bleiben,
# auch wenn sich der Quellcode ändert - ohne dass sie im finalen Image-Layer
# landen. Nur das explizit angegebene Binary wird gebaut, nicht der ganze
# Workspace - Cargo zieht sich die dafür nötigen Workspace-Member selbst.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/app/target \
    cargo build --release --bin ${BIN_NAME} ${CARGO_BUILD_ARGS} \
    && cp target/release/${BIN_NAME} /usr/src/app/${BIN_NAME}
# ^ Kopie nach außerhalb von target/, da der Inhalt von Cache-Mounts nach dem
#   RUN-Schritt nicht im Image-Layer landet und sonst verloren wäre.

########################################
# Stage 2: run (Default-Stage, da letzte im File)
########################################
FROM debian:bookworm-slim AS run
ARG BIN_NAME

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /app
# Ziel-Dateiname bewusst fest auf "app", damit ENTRYPOINT unten unabhängig
# von BIN_NAME immer passt.
COPY --from=build /usr/src/app/${BIN_NAME} /app/app
# Die statischen UI5-Assets gehören mit ins Image: ohne sie liefert der
# ServeDir-Fallback nur 404, und das Launchpad bleibt leer.
COPY web /app/web

# Schreibbare Verzeichnisse für Datenbank und Export. Beide sind als Volume
# gedacht - im Designer-Container liegt hier das Modell, im Laufzeit-Container
# die Fachdaten.
RUN mkdir -p /app/data /app/export && chown -R appuser /app/data /app/export

USER appuser
# Der Server liest PORT/DATA_DIR/EXPORT_DIR/WEB_ROOT aus der Umgebung
# (siehe fiori-server/src/config.rs); früher war 3033 hart verdrahtet und
# EXPOSE 8080 damit wirkungslos.
ENV PORT=8080 \
    DATA_DIR=/app/data \
    EXPORT_DIR=/app/export \
    WEB_ROOT=/app/web
EXPOSE 8080

ENTRYPOINT ["/app/app"]
