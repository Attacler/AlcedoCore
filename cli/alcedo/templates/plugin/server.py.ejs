import json
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path

        if path == "/health":
            self._json({"status": "healthy"})
        else:
            self._json({"status": "ok", "path": path})

    def do_POST(self):
        self._json({"status": "ok", "method": "POST"})

    def _json(self, data, status=200):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", 8080), Handler)
    print("Plugin running on port 8080")
    server.serve_forever()
