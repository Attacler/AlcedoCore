FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    docker.io \
    tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app -d /app -s /sbin/nologin app \
    && mkdir -p /app /plugins /var/lib/plugin-public \
    && chown -R app:app /app /plugins /var/lib/plugin-public

COPY target/release/dockerswarm /usr/local/bin/
COPY core-migrations /app/core-migrations/

ENV RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/usr/bin/tini", "--"]
USER app
CMD ["dockerswarm"]