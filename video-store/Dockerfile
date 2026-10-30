# Image fuer video-store (Laufzeit, Store: PostgreSQL ueber STORE_PG_URL):
#   /app/video-runtime   OData-Laufzeitserver (pjl-runtime + Video-Erweiterungen)
#
# Build-Kontext ist das Workspace-Wurzelverzeichnis (die Crates haengen per
# path aneinander). Daher wird aus video-store/ heraus mit `context: ..` gebaut.

########################################
# Stage 1: build
########################################
FROM rust:1-bookworm AS build
WORKDIR /usr/src/app
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/app/target \
    cargo build --release -p video-store --bin video-import \
 && cargo build --release -p video-store --bin video-runtime \
 && mkdir -p /out \
 && cp target/release/video-import target/release/video-runtime /out/

########################################
# Stage 2: run
########################################
FROM debian:bookworm-slim AS run

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    findutils \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /app
COPY --from=build /out/ /app/
COPY web /app/web

RUN mkdir -p /app/export && chown -R appuser /app/export

USER appuser
ENV PORT=8080 \
    EXPORT_DIR=/app/export \
    WEB_ROOT=/app/web \
    TERM3270_PORT=3270 \
    TERM3270_BIND=0.0.0.0
# 8080: OData/Launchpad, 3270: Statusansichten (Zugriff ueber Port-Mapping steuern)
EXPOSE 8080 3270
ENTRYPOINT ["/app/video-runtime"]
