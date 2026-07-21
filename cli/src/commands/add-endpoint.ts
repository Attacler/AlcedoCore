import { Command } from "commander";
import path from "node:path";
import fs from "node:fs";
import {
    createSpinner,
    success,
    error as logError,
    info,
} from "../utils/logger";
import { renderAndWrite } from "../utils/ejs-renderer";
import { loadConfig, ConfigSchema } from "../config";
import { assertSafeName } from "../utils/validation";
import {
    toPascalCase,
    toCamelCase,
    derivePath,
    deriveMethod,
} from "../utils/formatting";
import { generateTimestamp } from "../utils/generateTimestamp";

export const addEndpointCommand = new Command("endpoint")
    .argument("<name>", "Endpoint name (e.g., get_users)")
    .option(
        "-m, --method <method>",
        "HTTP method (GET, POST, PUT, DELETE, PATCH)",
    )
    .option("-p, --path <path>", "URL path (e.g., /api/users)")
    .description("Generate a handler stub and update manifest.json endpoints")
    .action(
        async (
            name: string,
            options: { method?: string; path?: string },
            cmd: Command,
        ) => {
            const config = loadConfig(
                cmd.optsWithGlobals() as Partial<ConfigSchema>,
            );

            assertSafeName(name, "endpoint name");

            const pluginDir = config.pluginDir || process.cwd();
            const manifestPath = path.resolve(pluginDir, "manifest.json");

            // Derive endpoint metadata
            const VALID_METHODS = [
                "GET",
                "POST",
                "PUT",
                "DELETE",
                "PATCH",
                "HEAD",
                "OPTIONS",
            ];
            const method = (options.method || deriveMethod(name)).toUpperCase();
            if (!VALID_METHODS.includes(method)) {
                throw new Error(
                    `Invalid HTTP method "${method}". Valid methods: ${VALID_METHODS.join(", ")}`,
                );
            }
            const urlPath = options.path || derivePath(name);
            const className = toPascalCase(name);
            const camelName = toCamelCase(name);
            const timestamp = generateTimestamp();

            // Detect language from existing server files
            let language: "python" | "node" = "python";
            if (fs.existsSync(path.join(pluginDir, "server.js"))) {
                language = "node";
            }

            const templatesDir = path.resolve(__dirname, "../../templates");
            const endpointsDir = path.resolve(pluginDir, "endpoints");

            const spinner = createSpinner(`Generating endpoint: ${name}`);

            try {
                // Create endpoints directory
                fs.mkdirSync(endpointsDir, { recursive: true });

                // Generate handler stub file
                const handlerTemplate =
                    language === "python" ? "handler.py.ejs" : "handler.js.ejs";
                const handlerFilename = `${name}.${language === "python" ? "py" : "js"}`;

                renderAndWrite(
                    path.join(templatesDir, "endpoint", handlerTemplate),
                    path.join(endpointsDir, handlerFilename),
                    {
                        name,
                        className,
                        camelName,
                        method,
                        path: urlPath,
                        timestamp,
                        language,
                    },
                );

                // Update manifest.json endpoints array
                if (!fs.existsSync(manifestPath)) {
                    throw new Error(
                        `No manifest.json found at ${manifestPath} — skipping manifest update.`,
                    );
                }

                const manifest = JSON.parse(
                    fs.readFileSync(manifestPath, "utf-8"),
                );

                // Initialize endpoints array if it doesn't exist
                if (!manifest.endpoints) {
                    manifest.endpoints = [];
                }

                // Append new endpoint entry (never modify existing entries — GEN-05)
                const newEntry = {
                    method,
                    path: urlPath,
                    description: `${camelName} endpoint`,
                    group: "api",
                };

                // Check for duplicate before appending (GEN-05 "never modify existing entries"
                // also implies no duplicate entries for the same method + path)
                const isDuplicate = manifest.endpoints.some(
                    (e: any) => e.method === method && e.path === urlPath,
                );

                if (isDuplicate) {
                    throw new Error(
                        `Endpoint ${method} ${urlPath} already exists in manifest.json — skipping.`,
                    );
                }
                manifest.endpoints.push(newEntry);

                // Write updated manifest.json back (preserving existing data)
                fs.writeFileSync(
                    manifestPath,
                    JSON.stringify(manifest, null, 2) + "\n",
                    "utf-8",
                );

                spinner.succeed();

                success(`Endpoint created:`);
                info(`  endpoints/${handlerFilename}`);
                info(`  manifest.json ← ${method} ${urlPath}`);
                info(
                    `Import the handler in your main server file and add the route.`,
                );
            } catch (err: any) {
                spinner.fail();
                logError(`Failed to generate endpoint: ${err.message}`);
                process.exit(1);
            }
        },
    );
