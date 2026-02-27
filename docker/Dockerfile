FROM rust:1.93-slim AS builder

WORKDIR /build

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY locales ./locales
COPY static ./static
COPY migrations ./migrations

RUN cargo build --release --locked --bin ropds

FROM debian:trixie-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        tzdata \
        curl \
        poppler-utils \
        djvulibre-bin \
        netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ropds /app/ropds

COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
COPY docker/wait-for-db.sh /usr/local/bin/wait-for-db.sh
RUN chmod +x /usr/local/bin/entrypoint.sh /usr/local/bin/wait-for-db.sh /app/ropds

ENV ROPDS_CONFIG=/app/config/config.toml
ENV ROPDS_DB_WAIT_TIMEOUT=60
ENV ROPDS_ADMIN_INIT_ONCE=true

EXPOSE 8081

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD []
