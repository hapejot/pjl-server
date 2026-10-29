# Workbench: generischer Laufzeitserver (pjl-runtime). Das Modell kommt aus workbench/ddic.json.
# Build-Kontext ist das Workspace-Wurzelverzeichnis; gebaut wird aus workbench/ mit `context: ..`.

FROM rust:1-bookworm AS build
WORKDIR /usr/src/app
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/app/target \
    cargo build --release -p fiori-server --bin pjl-runtime \
 && mkdir -p /out \
 && cp target/release/pjl-runtime /out/

FROM debian:bookworm-slim AS run

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /app
COPY --from=build /out/pjl-runtime /app/pjl-runtime
COPY web /app/web

RUN mkdir -p /app/data && chown -R appuser /app/data

USER appuser
ENV PORT=8080 \
    DATA_DIR=/app/data \
    WEB_ROOT=/app/web \
    TERM3270_PORT=3270 \
    TERM3270_BIND=0.0.0.0
EXPOSE 8080 3270
ENTRYPOINT ["/app/pjl-runtime"]
