import http from "node:http";
import type { AddressInfo } from "node:net";
import detectPort from "detect-port";

export const PROXY_PREFIX = "/api/internal-registry-proxy";

export interface RegistryForwarder {
    port: number;
    close: () => Promise<void>;
}

/**
 * Loopback HTTP server that makes real `docker push` work with a developer
 * API key. Docker talks plain HTTP to this server (localhost is treated as
 * insecure); we forward to the core's internal registry proxy, injecting the
 * dev key as a Bearer token.
 */
export async function startRegistryForwarder(opts: {
    coreUrl: string;
    apiKey: string;
    port?: number;
}): Promise<RegistryForwarder> {
    const core = new URL(opts.coreUrl);
    const listenPort =
        opts.port !== undefined ? opts.port : await detectPort(5001);
    let boundPort = listenPort;

    const server = http.createServer((clientReq, clientRes) => {
        const incomingPath = clientReq.url || "/";
        const targetPath = incomingPath.startsWith(PROXY_PREFIX)
            ? incomingPath
            : `${PROXY_PREFIX}${incomingPath}`;

        const headers: Record<string, any> = { ...clientReq.headers };
        headers["authorization"] = `Bearer ${opts.apiKey}`;
        headers["host"] = `localhost:${boundPort}`;
        for (const h of [
            "connection",
            "keep-alive",
            "proxy-authorization",
            "transfer-encoding",
            "upgrade",
        ]) {
            delete headers[h];
        }

        const proxyReq = http.request(
            {
                hostname: core.hostname,
                port: core.port || (core.protocol === "https:" ? 443 : 80),
                path: targetPath,
                method: clientReq.method,
                headers,
            },
            (proxyRes) => {
                const responseHeaders = { ...proxyRes.headers };
                delete responseHeaders["transfer-encoding"];
                delete responseHeaders["connection"];
                clientRes.writeHead(proxyRes.statusCode || 502, responseHeaders);
                proxyRes.pipe(clientRes);
            },
        );

        proxyReq.on("error", (err) => {
            if (!clientRes.headersSent) {
                clientRes.writeHead(502, { "Content-Type": "text/plain" });
            }
            clientRes.end(`Registry proxy error: ${err.message}`);
        });

        clientReq.pipe(proxyReq);
    });

    await new Promise<void>((resolve, reject) => {
        server.once("error", reject);
        server.listen(listenPort, "127.0.0.1", () => {
            boundPort = (server.address() as AddressInfo).port;
            resolve();
        });
    });

    return {
        port: boundPort,
        close: () =>
            new Promise<void>((resolve) => {
                server.close(() => resolve());
            }),
    };
}
