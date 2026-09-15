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

USER appuser
ENV PORT=8080
EXPOSE 8080

ENTRYPOINT ["/app/app"]
