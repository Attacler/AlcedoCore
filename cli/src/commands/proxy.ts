import { Command } from "commander";
import http from "node:http";
import path from "node:path";
import { loadConfig } from "../config";
import {
    success,
    error as logError,
    info,
    warn,
    createSpinner,
} from "../utils/logger";

interface ProxyOptions {
    port?: string;
    target?: string;
    apiKey?: string;
}

export const proxyCommand = new Command("proxy")
    .description(
        "Start a dev proxy that forwards requests to a local dev server",
    )
    .option("-p, --port <port>", "Proxy listen port", "3099")
    .option(
        "-t, --target <url>",
        "Target dev server URL (e.g., localhost:8080)",
        "localhost:3000",
    )
    .option("-k, --api-key <key>", "Core API key (or ALCEDO_API_KEY env var)")
    .action(async (opts: ProxyOptions, cmd: Command) => {
        const config = loadConfig(cmd.optsWithGlobals() as any);
        const coreUrl = config.coreUrl || "http://localhost:8080";
        const pluginDir = path.resolve(config.pluginDir || process.cwd());
        const slug = path.basename(pluginDir);
        const apiKey =
            opts.apiKey || config.apiKey || process.env.ALCEDO_API_KEY || "";
        const proxyPort = parseInt(opts.port || "3099", 10);
        const target = opts.target || "localhost:3000";

        if (isNaN(proxyPort) || proxyPort < 1 || proxyPort > 65535) {
            logError(`Invalid proxy port: ${opts.port}`);
            process.exit(1);
        }

        const parsedTarget = parseTarget(target);

        const validateSpinner = createSpinner("Validating API key...");
        const testId = await registerRequest(
            coreUrl,
            apiKey || undefined,
            slug,
        );
        if (!testId) {
            validateSpinner.fail();
            logError(`Failed to connect to core at ${coreUrl}`);
            logError(
                "Make sure the AlcedoCore instance is running and API key is correct",
            );
            process.exit(1);
        }
        validateSpinner.succeed();

        const server = http.createServer(async (clientReq, clientRes) => {
            const startTime = Date.now();

            const requestId = await registerRequest(
                coreUrl,
                apiKey || undefined,
                slug,
            );
            if (!requestId) {
                logError("Failed to register request ID with core");
                clientRes.statusCode = 502;
                clientRes.setHeader("Content-Type", "text/plain");
                clientRes.end("Bad Gateway: core unavailable");
                return;
            }

            // Clone headers and inject X-Request-ID
            const headers: Record<string, string> = {};
            for (const [key, value] of Object.entries(clientReq.headers)) {
                if (value !== undefined) {
                    headers[key] = Array.isArray(value)
                        ? value.join(", ")
                        : value;
                }
            }
            headers["X-Request-ID"] = requestId;
            headers["host"] = `${parsedTarget.hostname}:${parsedTarget.port}`;

            const options: http.RequestOptions = {
                hostname: parsedTarget.hostname,
                port: parsedTarget.port,
                path: clientReq.url,
                method: clientReq.method,
                headers,
            };

            const proxyReq = http.request(options, (proxyRes) => {
                const chunks: Buffer[] = [];
                proxyRes.on("data", (chunk: Buffer) => chunks.push(chunk));
                proxyRes.on("end", () => {
                    const duration = Date.now() - startTime;
                    const statusCode = proxyRes.statusCode || 0;

                    info(
                        `  ${statusCode} ${clientReq.method} ${duration}ms ${clientReq.url}`,
                    );

                    const responseHeaders = { ...proxyRes.headers };
                    clientRes.writeHead(statusCode, responseHeaders);
                    clientRes.end(Buffer.concat(chunks));
                });
            });

            proxyReq.on("error", (err) => {
                logError(`Proxy request error: ${err.message}`);
                if (!clientRes.headersSent) {
                    clientRes.statusCode = 502;
                    clientRes.setHeader("Content-Type", "text/plain");
                    clientRes.end(`Bad Gateway: ${err.message}`);
                }
            });

            clientReq.pipe(proxyReq);
        });

        let shuttingDown = false;

        function handleShutdown() {
            if (shuttingDown) return;
            shuttingDown = true;
            console.log("");
            info("Shutting down proxy...");
            server.close(() => {
                success("Proxy stopped");
                process.exit(0);
            });
            setTimeout(() => {
                warn("Proxy did not close gracefully, forcing exit");
                process.exit(0);
            }, 3000);
        }

        process.on("SIGINT", handleShutdown);
        process.on("SIGTERM", handleShutdown);
        process.on("SIGHUP", handleShutdown);

        server.listen(proxyPort, () => {
            success(`Dev proxy listening on http://localhost:${proxyPort}`);
            info(`Forwarding to http://${target}`);
            info(`Plugin slug: ${slug}`);
            info(`Core URL: ${coreUrl}`);
            if (apiKey) {
                info("API key authentication enabled");
            }
            info("Press Ctrl+C to stop");
        });
    });

async function coreFetch(
    coreUrl: string,
    apiKey: string | undefined,
    endpoint: string,
    body: Record<string, unknown>,
): Promise<Response | null> {
    try {
        const headers: Record<string, string> = {
            "Content-Type": "application/json",
        };
        if (apiKey) {
            headers["Authorization"] = `Bearer ${apiKey}`;
        }
        return await fetch(`${coreUrl.replace(/\/$/, "")}${endpoint}`, {
            method: "POST",
            headers,
            body: JSON.stringify(body),
        });
    } catch {
        return null;
    }
}

async function registerRequest(
    coreUrl: string,
    apiKey: string | undefined,
    slug: string,
): Promise<string | null> {
    const res = await coreFetch(coreUrl, apiKey, "/api/dev/request-id", {
        slug,
    });
    if (!res || !res.ok) return null;
    try {
        const data = (await res.json()) as { request_id: string };
        return data.request_id;
    } catch {
        return null;
    }
}

function parseTarget(target: string): { hostname: string; port: number } {
    let cleaned = target.replace(/^https?:\/\//, "");
    const [hostname, portStr] = cleaned.split(":");
    const port = portStr ? parseInt(portStr, 10) : 3000;
    return { hostname, port };
}
