FROM python:3.14

WORKDIR /app

COPY requirements.txt .
RUN pip install -r requirements.txt

COPY server.py .
COPY public/ ./public/
COPY docs/ ./docs/
COPY migrations/ ./migrations/
COPY pages/dist/ ./pages/dist/
COPY manifest.json ./manifest.json

# EXPOSE 8080

CMD ["python", "server.py"]
