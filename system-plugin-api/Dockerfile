# Use a Rust-compatible base image
FROM rust:1.97.1-slim as builder

# Set the working directory
WORKDIR /usr/src/alcedo-system-plugin-api

# Copy the project files
COPY . .

# Build the project in release mode
RUN cargo build --release

# Use a minimal base image for the final stage
FROM debian:bookworm-slim

# Install dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/alcedo-system-plugin-api/target/release/alcedo-system-plugin-api /usr/local/bin/alcedo-system-plugin-api

# Copy the plugins.json file
COPY plugins.json /app/plugins.json

# Expose the port
EXPOSE 9090

# Set environment variables
ENV PLUGINS_DIR=/app
ENV PORT=9090

# Run the binary
CMD ["alcedo-system-plugin-api"]