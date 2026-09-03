import { Command } from "commander";
import { createSpinner, success, error as logError } from "../utils/logger";
import path from "path";
import { loadConfig } from "../config";
import { existsSync, readFileSync } from "fs";
import { execSync } from "child_process";

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

        try {
            spinner.text = "Building Docker image...";
            await buildDockerImage(
                pluginDir,
                config.registryUrl,
                manifest.name,
                manifest.version,
            );
            spinner.text = "Building complete, pushing Docker image...";
            await pushDockerImage(
                config.registryUrl,
                manifest.name,
                manifest.version,
            );
            spinner.succeed();
            success(
                `Plugin has been build and pushed under ${manifest.name}:${manifest.version}`,
            );
        } catch (err: any) {
            spinner.fail();
            logError(`Failed to deploy: ${err.message}`);
            process.exit(1);
        }
    });

function buildDockerImage(
    pluginDir: string,
    registryURL: string,
    image: string,
    version: string,
): boolean {
    execSync(
        `docker build -t ${registryURL}/${image}:${version} ${pluginDir}`,
        {
            stdio: "pipe",
        },
    );
    return true;
}

function pushDockerImage(
    registryURL: string,
    image: string,
    version: string,
): boolean {
    execSync(`docker push ${registryURL}/${image}:${version}`, {
        stdio: "pipe",
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
