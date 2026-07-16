FROM python:3.11-slim

WORKDIR /app

COPY sdk/python/ ./sdk/
RUN pip install ./sdk/ aiohttp

COPY sample-plugins/hello-world/server.py .
COPY sample-plugins/hello-world/public/ ./public/
COPY sample-plugins/hello-world/docs/ ./docs/
COPY sample-plugins/hello-world/migrations/ ./migrations/
COPY sample-plugins/hello-world/pages/dist/ /pages/dist/
COPY sample-plugins/hello-world/manifest.json /app/manifest.json

EXPOSE 8080

CMD ["python", "server.py"]
