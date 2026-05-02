# check=skip=SecretsUsedInArgOrEnv
# SSHOOSH_SERVER_KEY stores the host-key path, not secret key material.
FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 sshoosh \
    && useradd --system --uid 10001 --gid sshoosh --home-dir /data --shell /usr/sbin/nologin sshoosh \
    && install -d -m 0700 -o sshoosh -g sshoosh /data

COPY --from=builder /app/target/release/sshoosh /usr/local/bin/sshoosh

ENV SSHOOSH_DB=/data/sshoosh.sqlite \
    SSHOOSH_SERVER_KEY=/data/sshoosh_server_ed25519 \
    SSHOOSH_HOST=0.0.0.0 \
    SSHOOSH_PORT=2222

VOLUME ["/data"]
EXPOSE 2222

USER sshoosh:sshoosh
ENTRYPOINT ["sshoosh"]
CMD ["serve"]
