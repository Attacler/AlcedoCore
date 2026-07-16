# Stage 1: Build the plugin (including the Rust SDK as a path dependency)
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build

# Copy SDK and plugin source, preserving relative path structure
COPY sdk/ ./sdk/
COPY sample-plugins/hello-world-rust/ ./sample-plugins/hello-world-rust/

# Build the plugin in release mode
RUN cargo build --release --manifest-path ./sample-plugins/hello-world-rust/Cargo.toml

# Stage 2: Minimal runtime image
FROM gcr.io/distroless/cc-debian12

WORKDIR /app

# Copy the compiled binary
COPY --from=builder /build/sample-plugins/hello-world-rust/target/release/hello-world-rust /app/hello-world-rust

# Copy runtime assets
COPY sample-plugins/hello-world-rust/manifest.json /app/manifest.json
COPY sample-plugins/hello-world-rust/migrations/ /app/migrations/
COPY sample-plugins/hello-world-rust/pages/ ./pages/

EXPOSE 8080

CMD ["/app/hello-world-rust"]
