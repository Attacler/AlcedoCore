import { Command } from "commander";
import fs from "node:fs";
import path from "node:path";
import { loadConfig } from "../config";
import {
    success,
    error as logError,
    info,
    createSpinner,
} from "../utils/logger";

interface ConnectOptions {
    url?: string;
    apiKey?: string;
    app?: string;
    version?: string;
}

export const connectCommand = new Command("connect")
    .description("Connect to a core instance and save credentials")
    .option("-u, --url <url>", "Core URL")
    .option("-k, --api-key <key>", "API key (version-scoped)")
    .option("--app <app>", "App to use during development")
    .option("--version <version>", "Version the API key is scoped to")
    .action(async (opts: ConnectOptions, cmd: Command) => {
        const config = loadConfig(cmd.optsWithGlobals() as any);

        let coreUrl = opts.url || config.coreUrl || "http://localhost:8080";
        let apiKey = opts.apiKey || "";
        let app = opts.app || config.app || "";
        let version = opts.version || config.version || "";

        // If no flags, prompt interactively
        if (!opts.url || !opts.apiKey || !opts.app || !opts.version) {
            const inquirer = (await import("inquirer")).default;
            const prompts: any[] = [];

            if (!opts.url) {
                prompts.push({
                    type: "input",
                    name: "coreUrl",
                    message: "Core URL:",
                    default: coreUrl,
                });
            }
            if (!opts.apiKey) {
                prompts.push({
                    type: "password",
                    name: "apiKey",
                    message: "API Key:",
                    mask: "*",
                });
            }
            if (!opts.app) {
                prompts.push({
                    type: "input",
                    name: "app",
                    message: "App:",
                    default: app,
                });
            }
            if (!opts.version) {
                prompts.push({
                    type: "input",
                    name: "version",
                    message: "Version (must match the API key's version):",
                    default: version,
                });
            }

            const answers = await inquirer.prompt(prompts);
            if (answers.coreUrl) coreUrl = answers.coreUrl;
            if (answers.apiKey) apiKey = answers.apiKey;
            if (answers.app) app = answers.app;
            if (answers.version) version = answers.version;
        }

        if (!coreUrl) {
            logError("Core URL is required");
            process.exit(1);
        }
        if (!apiKey) {
            logError("API key is required");
            process.exit(1);
        }
        if (!app || !version) {
            logError("App and version are required (the API key is version-scoped)");
            process.exit(1);
        }

        // Validate by registering a test request ID
        const validateSpinner = createSpinner("Validating connection...");
        try {
            const res = await fetch(
                `${coreUrl.replace(/\/$/, "")}/api/dev/request-id`,
                {
                    method: "POST",
                    headers: {
                        "Content-Type": "application/json",
                        Authorization: `Bearer ${apiKey}`,
                        "X-App": app,
                        "X-Version": version,
                    },
                    body: JSON.stringify({ slug: "alcedo-connect" }),
                },
            );
            if (!res.ok) {
                let errMsg = `HTTP ${res.status}`;
                try {
                    const errBody = (await res.json()) as { error?: string };
                    if (errBody.error) errMsg = errBody.error;
                } catch {}
                throw new Error(errMsg);
            }
            validateSpinner.succeed();
        } catch (err: any) {
            validateSpinner.fail();
            logError(`Failed to connect: ${err.message}`);
            process.exit(1);
        }

        // Write to .alcedocore.dev.env
        const envPath = path.join(process.cwd(), ".alcedocore.dev.env");
        fs.writeFileSync(
            envPath,
            `CORE_URL=${coreUrl}\nAPI_KEY=${apiKey}\nAPP=${app}\nVERSION=${version}\n`,
        );
        success(`Credentials saved to ${envPath}`);
        info(`  CORE_URL=${coreUrl}`);
        info(`  APP=${app}`);
        info(`  VERSION=${version}`);
        info("  API_KEY=********");
    });
