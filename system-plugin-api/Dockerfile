FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/release/system-plugin-api /usr/local/bin/system-plugin-api
COPY plugins.json /app/plugins.json
EXPOSE 9090
ENV PLUGINS_DIR=/app
ENV PORT=9090
CMD ["system-plugin-api"]
