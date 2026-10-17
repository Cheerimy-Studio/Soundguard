# syntax=docker/dockerfile:1

FROM node:22-bookworm AS web
WORKDIR /src
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS server
WORKDIR /src
COPY server/ ./server/
RUN cargo build --release --manifest-path server/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app soundguard \
    && mkdir -p /app/web /data \
    && chown -R soundguard:soundguard /app /data

WORKDIR /app
COPY --from=server /src/server/target/release/soundguard-server /app/soundguard-server
COPY --from=web /src/web/dist /app/web/dist

ENV DATA_DIR=/data \
    WEB_DIST=/app/web/dist \
    BIND_ADDR=0.0.0.0 \
    PORT=8080

USER soundguard
VOLUME ["/data"]
EXPOSE 8080

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/soundguard-server"]
