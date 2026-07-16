FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/release/automation-plugin /app/automation-plugin
COPY migrations/ /app/migrations/
COPY pages/dist/ /app/pages/dist/
COPY manifest.json /app/manifest.json
EXPOSE 8080
CMD ["/app/automation-plugin"]
