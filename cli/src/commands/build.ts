import { Command } from "commander";
import {
    createSpinner,
    success,
    error as logError,
    info,
} from "../utils/logger";
import path from "path";
import { ConfigSchema, loadConfig } from "../config";
import { existsSync, readFileSync } from "fs";
import { execSync } from "child_process";

export const buildCommand = new Command("build")
    .description("Build a plugin as a Docker image")
    .action(async (slug: string, options: any, cmd: Command) => {
        const spinner = createSpinner(`Preparing build...`);
        const config = loadConfig();
        // cmd.optsWithGlobals() as Partial<ConfigSchema>,

        const pluginDir = config.pluginDir || process.cwd();
        const manifestPath = path.resolve(pluginDir, "manifest.json");
        const dockerFilePath = path.resolve(pluginDir, "Dockerfile");

        if (!existsSync(manifestPath)) {
            spinner.fail();
            logError(`Could not build, manifest.json is missing!`);
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
            await buildDockerImage(pluginDir, manifest.name, manifest.version);
            spinner.succeed();
            success(
                `Plugin "${slug}" has been build under ${manifest.name}:${manifest.version}`,
            );
        } catch (err: any) {
            spinner.fail();
            logError(`Failed to deploy: ${err.message}`);
            process.exit(1);
        }
    });

function buildDockerImage(
    pluginDir: string,
    image: string,
    version: string,
): boolean {
    console.log(`docker build ${pluginDir} ${image}:${version}`);
    execSync(`docker build -t ${image}:${version} ${pluginDir}`, {
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
