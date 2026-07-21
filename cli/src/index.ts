#!/usr/bin/env node
import { Command } from "commander";
import { loadConfig, ConfigSchema } from "./config";
import { error as logError } from "./utils/logger";
import { initCoreCommand } from "./commands/init-core";
import { initCommand } from "./commands/init";
import { deployCommand } from "./commands/deploy";
import { addMigrationCommand } from "./commands/add-migration";
import { migrateCommand } from "./commands/migrate";
import { addEndpointCommand } from "./commands/add-endpoint";
import { addPageCommand } from "./commands/add-page";
import { addNavItemCommand } from "./commands/add-nav-item";
import { connectCommand } from "./commands/connect";
import fs from "node:fs";
import path from "node:path";
import { devCommand } from "./commands/dev";

// Read version from package.json
function getVersion(): string {
    const pkgPath = path.resolve(__dirname, "../package.json");
    const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));
    return pkg.version;
}

const program = new Command();

program
    .name("alcedo")
    .version(getVersion(), "-V, --version", "Output the version number")
    .description(
        "Alcedo plugin development CLI - scaffold, develop, and test plugins",
    )
    .hook("preAction", (thisCommand) => {
        // Parse config before any action runs
        // Walk parent chain to collect all options (including global flags)
        const opts: Partial<ConfigSchema> = {};
        let cmd: Command | null = thisCommand;
        while (cmd) {
            Object.assign(opts, cmd.opts());
            cmd = cmd.parent as Command | null;
        }
        const config = loadConfig(opts);
        // Store config on the command for subcommands to access
        (thisCommand as any)._alcedoConfig = config;
    });

// Global options
program.option(
    "-r, --registry-url <url>",
    "Docker registry URL (default: localhost:5000)",
);
program.option(
    "-c, --core-url <url>",
    "core API URL (default: http://localhost:8080)",
);
program.option(
    "-d, --plugin-dir <path>",
    "Plugin project directory (default: current directory)",
);

program.addCommand(initCommand);
program.addCommand(initCoreCommand);
program.addCommand(deployCommand);

const addCommand = new Command("add").description(
    "Generate plugin components (migrations, endpoints, pages, nav-items)",
);

addCommand.addCommand(addMigrationCommand);
addCommand.addCommand(addEndpointCommand);
addCommand.addCommand(addPageCommand);
addCommand.addCommand(addNavItemCommand);

program.addCommand(addCommand);
program.addCommand(migrateCommand);
program.addCommand(connectCommand);
program.addCommand(devCommand);

// Error handling: non-zero exit on errors (CLI-04)
// Catch unhandled promise rejections and exceptions
process.on("unhandledRejection", (reason) => {
    logError(`Unhandled error: ${reason}`);
    process.exit(1);
});

process.on("uncaughtException", (err) => {
    logError(`Fatal error: ${err.message}`);
    process.exit(1);
});

async function main(): Promise<void> {
    await program.parseAsync(process.argv);
}

main().catch((err) => {
    logError(`Fatal error: ${err.message}`);
    process.exit(1);
});
