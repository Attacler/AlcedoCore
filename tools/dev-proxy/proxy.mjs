#!/usr/bin/env node
import http from "node:http";
import net from "node:net";

const CORE_URL = process.env.DEV_ALCEDO_CORE_URL;
const TARGET_PORT = parseInt(process.env.DEV_TARGET_PORT || "", 10);

if (!CORE_URL || !TARGET_PORT) {
  console.error("Usage: DEV_ALCEDO_CORE_URL=http://localhost:8080 DEV_TARGET_PORT=3000 node proxy.mjs");
  process.exit(1);
}

const coreBase = CORE_URL.replace(/\/$/, "");

// Helper: fetch + extract X-Request-ID from response headers
async function fetchRequestId() {
  const res = await fetch(`${coreBase}/health`);
  if (!res.ok) throw new Error(`Health check failed: ${res.status}`);
  return res.headers.get("x-request-id") || crypto.randomUUID();
}

// Find next available port starting from `start`
function findPort(start) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.on("error", () => resolve(findPort(start + 1)));
    srv.listen(start, "127.0.0.1", () => {
      srv.close(() => resolve(start));
    });
  });
}

// Startup sequence
const proxyPort = await findPort(3000);

console.log(`[dev-proxy] Checking alcedocore at ${coreBase}...`);
try {
  const hc = await fetch(`${coreBase}/health`);
  if (!hc.ok) throw new Error(`HTTP ${hc.status}`);
  console.log(`[dev-proxy] AlcedoCore reachable`);
} catch (err) {
  console.error(`[dev-proxy] Cannot reach ${coreBase}: ${err.message}`);
  process.exit(1);
}

console.log(`[dev-proxy] Checking target localhost:${TARGET_PORT}...`);
try {
  await new Promise((resolve, reject) => {
    const sock = net.createConnection(TARGET_PORT, "127.0.0.1", () => {
      sock.end();
      resolve();
    });
    sock.on("error", reject);
  });
  console.log(`[dev-proxy] Target reachable`);
} catch {
  console.error(`[dev-proxy] Cannot connect to localhost:${TARGET_PORT} — is your plugin running?`);
  process.exit(1);
}

let currentId = await fetchRequestId();
console.log(`[dev-proxy] Initial X-Request-ID: ${currentId}`);

setInterval(async () => {
  try {
    currentId = await fetchRequestId();
    console.log(`[dev-proxy] Refreshed X-Request-ID: ${currentId}`);
  } catch (err) {
    console.error(`[dev-proxy] Refresh failed: ${err.message}`);
  }
}, 10 * 60 * 1000);

// HTTP proxy
const server = http.createServer((clientReq, clientRes) => {
  const options = {
    hostname: "127.0.0.1",
    port: TARGET_PORT,
    path: clientReq.url,
    method: clientReq.method,
    headers: {
      ...clientReq.headers,
      "x-request-id": currentId,
      "x-forwarded-for": clientReq.socket.remoteAddress || "127.0.0.1",
      "x-forwarded-host": clientReq.headers.host || "",
    },
  };

  const proxyReq = http.request(options, (proxyRes) => {
    const { statusCode, statusMessage, headers } = proxyRes;
    clientRes.writeHead(statusCode, statusMessage, headers);
    proxyRes.pipe(clientRes);
  });

  proxyReq.on("error", (err) => {
    console.error(`[dev-proxy] ${clientReq.method} ${clientReq.url} — ${err.message}`);
    clientRes.writeHead(502, { "content-type": "text/plain" });
    clientRes.end(`Dev proxy error: ${err.message}`);
  });

  clientReq.pipe(proxyReq);
});

server.listen(proxyPort, "127.0.0.1", () => {
  console.log(`[dev-proxy] Listening on http://127.0.0.1:${proxyPort}`);
  console.log(`[dev-proxy] Forwarding to http://127.0.0.1:${TARGET_PORT}`);
  console.log(`[dev-proxy] X-Request-ID refreshes every 10 minutes`);
  console.log(`[dev-proxy] Ready`);
});
