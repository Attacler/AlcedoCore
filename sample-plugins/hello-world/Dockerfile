FROM python:3.14

ENV PYTHONUNBUFFERED=1

WORKDIR /app

COPY requirements.txt .
RUN pip install -r requirements.txt

COPY server.py .
COPY public/ ./public/
COPY docs/ ./docs/
COPY migrations/ ./migrations/
COPY manifest.json ./manifest.json


COPY pages/ ./pages/
RUN test -f pages/dist/plugin-pages.js || { \
        echo "pages/dist/plugin-pages.js is missing." >&2; \
        echo "Run 'npm run build:pages' in this directory" >&2; \
        echo "(needs cli/ built first: cd ../../cli && npm install && npm run build)" >&2; \
        exit 1; \
    }

CMD ["python", "server.py"]
