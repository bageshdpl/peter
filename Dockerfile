# Peter as one container: the API server, the web app it serves, and the
# extension it offers for download. Used by Render (see render.yaml) and any
# other Docker host. Data lives in /data; set the BACKUP_S3_* variables to keep
# it safe on hosts whose disk is wiped on restart (see docs/DEPLOY.md).

FROM rust:1.97-bookworm AS server
WORKDIR /src
COPY server/ .
RUN cargo build --release --locked

FROM node:22-bookworm-slim AS web
WORKDIR /src
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ .
RUN npm run build

# The backup tool, fetched in a stage that already has curl and certificates.
FROM rust:1.97-bookworm AS tools
ARG TARGETARCH=amd64
# Litestream (https://litestream.io, Apache-2.0), pinned by version and checksum.
ARG LITESTREAM_VERSION=0.3.13
RUN case "$TARGETARCH" in \
      amd64) sum=eb75a3de5cab03875cdae9f5f539e6aedadd66607003d9b1e7a9077948818ba0 ;; \
      arm64) sum=9585f5a508516bd66af2b2376bab4de256a5ef8e2b73ec760559e679628f2d59 ;; \
      *) echo "unsupported architecture $TARGETARCH" && exit 1 ;; \
    esac \
 && curl -fsSL -o /tmp/litestream.tar.gz \
    "https://github.com/benbjohnson/litestream/releases/download/v${LITESTREAM_VERSION}/litestream-v${LITESTREAM_VERSION}-linux-${TARGETARCH}.tar.gz" \
 && echo "$sum  /tmp/litestream.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/litestream.tar.gz -C /usr/local/bin litestream

FROM debian:bookworm-slim
# Certificates for calling AI providers and the backup storage over HTTPS.
COPY --from=tools /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=tools /usr/local/bin/litestream /usr/local/bin/litestream
RUN useradd --create-home --uid 10001 peter && mkdir /data && chown peter /data

WORKDIR /app
COPY --from=server /src/target/release/peter-server /app/peter-server
COPY --from=web /src/dist /app/web
COPY extension /app/extension
COPY deploy/entrypoint.sh deploy/litestream.yml /app/

ENV PETER_WEB_DIR=/app/web \
    PETER_EXTENSION_DIR=/app/extension \
    PETER_DB_PATH=/data/peter.db \
    PETER_DATA_DIR=/data/files \
    PETER_ALLOW_SIGNUP=false

USER peter
EXPOSE 8080
CMD ["/app/entrypoint.sh"]
