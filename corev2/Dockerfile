FROM lukemathwalker/cargo-chef:latest-rust-1.97.1-slim-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang \
        mold \
        pkg-config \
        libssl-dev \
        build-essential \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY . /workspace/backend/
WORKDIR /workspace/backend
RUN cargo chef prepare --recipe-path /workspace/recipe.json

FROM chef AS builder
COPY --from=planner /workspace/recipe.json /workspace/recipe.json
WORKDIR /workspace/backend

COPY --from=planner /workspace/backend/file-storage ./file-storage
COPY --from=planner /workspace/backend/file-storage-local ./file-storage-local
COPY --from=planner /workspace/backend/file-storage-s3 ./file-storage-s3
RUN cargo chef cook --release --recipe-path /workspace/recipe.json \
    --package backend --features docker
COPY . /workspace/backend/
RUN cargo build --release --package backend --features docker

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app -d /app -s /sbin/nologin app \
    && mkdir -p /app /plugins /var/lib/plugin-public \
    && chown -R app:app /app /plugins /var/lib/plugin-public

COPY --from=builder /workspace/backend/target/release/backend /usr/local/bin/backend

ENV RUST_LOG=info
WORKDIR /app
EXPOSE 8080

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["backend"]