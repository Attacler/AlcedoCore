# syntax=docker/dockerfile:1

FROM lukemathwalker/cargo-chef:latest-rust-1.97.1-slim-bookworm AS chef
WORKDIR /workspace

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY core/ /workspace/core/
WORKDIR /workspace/core
RUN cargo chef prepare --recipe-path /workspace/recipe.json

FROM chef AS builder
WORKDIR /workspace/core
COPY --from=planner /workspace/recipe.json /workspace/recipe.json
# Compile dependencies only - this is the caching Docker layer
RUN cargo chef cook --release --recipe-path /workspace/recipe.json \
    --package bins --features k8s

# Build the real source on top of the cached dependencies
COPY core/ /workspace/core/
RUN cargo build --release -p bins --bins --features k8s

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    libssl3 \
    tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app -d /app -s /sbin/nologin app \
    && mkdir -p /app /plugins /var/lib/plugin-public \
    && chown -R app:app /app /plugins /var/lib/plugin-public

COPY --from=builder /workspace/core/target/release/dockerswarm /usr/local/bin/
COPY core/core-migrations /app/core-migrations/

ENV RUST_LOG=info
 
EXPOSE 8080

ENTRYPOINT ["/usr/bin/tini", "--"]
# USER app
CMD ["dockerswarm"]