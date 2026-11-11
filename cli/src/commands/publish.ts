import { Command } from "commander";
import { createSpinner, success, error as logError } from "../utils/logger";
import path from "path";
import { loadConfig } from "../config";
import { existsSync, readFileSync } from "fs";
import { exec, execSync } from "child_process";
import { promisify } from "util";
import { startRegistryForwarder } from "../utils/registryProxy";

const execAsync = promisify(exec);

export const publishCommand = new Command("publish")
    .description("Build and push a plugin to an image registry")
    .action(async (slug: string, options: any, cmd: Command) => {
        const spinner = createSpinner(`Preparing build...`);
        const config = loadConfig();

        const pluginDir = config.pluginDir || process.cwd();
        const manifestPath = path.resolve(pluginDir, "manifest.json");
        const dockerFilePath = path.resolve(pluginDir, "Dockerfile");

        if (!existsSync(manifestPath)) {
            spinner.fail();
            logError(`Could not build, manifest.json is missing!`);
            process.exit(1);
        }

        if (!config.registryUrl) {
            spinner.fail();
            logError(`Registery URL not set`);
            process.exit(1);
        }

        if (!existsSync(dockerFilePath)) {
            spinner.fail();
            logError(`Could not build, Dockerfile is missing!`);
            process.exit(1);
        }

        if (!checkDocker) {
            spinner.fail();
            logError(`Could not build, Docker is unreachable!`);
            process.exit(1);
        }

        const manifest = JSON.parse(readFileSync(manifestPath, "utf-8"));

        const coreUrl = config.coreUrl || "http://localhost:8080";
        const apiKey = config.apiKey || process.env.ALCEDO_API_KEY;
        if (!apiKey) {
            spinner.fail();
            logError(
                "Developer API key required. Run `alcedo connect` or set ALCEDO_API_KEY.",
            );
            process.exit(1);
        }

        let forwarder: Awaited<ReturnType<typeof startRegistryForwarder>> | undefined;
        try {
            forwarder = await startRegistryForwarder({ coreUrl, apiKey });
            const registryHost = `localhost:${forwarder.port}`;

            spinner.text = "Building Docker image...";
            await buildDockerImage(
                pluginDir,
                registryHost,
                manifest.name,
                manifest.version,
            );
            spinner.text = "Building complete, pushing Docker image...";
            await pushDockerImage(registryHost, manifest.name, manifest.version);
            spinner.succeed();
            success(
                `Plugin has been build and pushed under ${manifest.name}:${manifest.version}`,
            );
        } catch (err: any) {
            spinner.fail();
            logError(`Failed to deploy: ${err.message}`);
            if (forwarder) await forwarder.close();
            process.exit(1);
        } finally {
            if (forwarder) await forwarder.close();
        }
    });

async function buildDockerImage(
    pluginDir: string,
    registryURL: string,
    image: string,
    version: string,
): Promise<boolean> {
    await execAsync(
        `docker build -t ${registryURL}/${image}:${version} ${pluginDir}`,
        {
            maxBuffer: 64 * 1024 * 1024,
        },
    );
    return true;
}

async function pushDockerImage(
    registryURL: string,
    image: string,
    version: string,
): Promise<boolean> {
    await execAsync(`docker push ${registryURL}/${image}:${version}`, {
        maxBuffer: 64 * 1024 * 1024,
    });
    return true;
}

function checkDocker(): boolean {
    try {
        execSync("docker info --format '{{.ServerVersion}}'", {
            stdio: "pipe",
            timeout: 5000,
        });
        return true;
    } catch {
        return false;
    }
}
