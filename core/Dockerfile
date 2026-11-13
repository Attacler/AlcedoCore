ARG RUST_VERSION=1.96.0

FROM lukemathwalker/cargo-chef:latest-rust-${RUST_VERSION}-slim-bookworm AS chef
# .cargo/config.toml forces clang + mold as the linker on Linux.
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang \
        mold \
        pkg-config \
        libssl-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
WORKDIR /workspace/backend
COPY . .
RUN cargo chef prepare --recipe-path /workspace/recipe.json

FROM chef AS builder
WORKDIR /workspace/backend
COPY --from=planner /workspace/recipe.json /workspace/recipe.json
# Path dependencies must exist before `cook` builds the dependency graph.
COPY Cargo.toml Cargo.lock ./
COPY file-storage ./file-storage
COPY file-storage-local ./file-storage-local
COPY file-storage-s3 ./file-storage-s3
RUN cargo chef cook --release --locked --recipe-path /workspace/recipe.json \
        --package backend --features docker
COPY . .
RUN cargo build --release --locked --package backend --features docker

FROM debian:bookworm-slim AS runtime
ARG DOCKER_GID=0
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app \
    && useradd -r -g app -d /app -s /sbin/nologin app \
    && mkdir -p /app \
    && chown -R app:app /app \
    && if [ "$DOCKER_GID" != "0" ]; then \
           groupadd -g "$DOCKER_GID" docker && usermod -aG docker app; \
       fi

COPY --from=builder /workspace/backend/target/release/backend /usr/local/bin/backend

ENV RUST_LOG=info
WORKDIR /app
USER app
EXPOSE 8080
# CORE_PORT defaults to 8080 in config.rs; the environment usually overrides it.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://localhost:${CORE_PORT:-8080}/health" || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["backend"]
