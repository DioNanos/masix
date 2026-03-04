FROM rust:bookworm AS builder

WORKDIR /src
COPY . .
RUN cargo build --release -p masix-cli --features minimal

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 masix \
    && useradd --uid 1000 --gid 1000 --home /home/masix --create-home --shell /usr/sbin/nologin masix

COPY --from=builder /src/target/release/masix /usr/local/bin/masix
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
COPY docker/memory /opt/masix/memory

RUN chmod 0755 /usr/local/bin/masix /usr/local/bin/entrypoint.sh

ENV MASIX_DATA_DIR=/data
ENV MASIX_CONFIG_FILE=/config/config.toml

USER masix
WORKDIR /home/masix

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
