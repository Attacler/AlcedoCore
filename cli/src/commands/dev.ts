import { Command } from "commander";
import { proxyCommand } from "./proxy";
import { buildFrontendCommand } from "./build-frontend";
import { serveFrontendCommand } from "./serve-frontend";

export const devCommand = new Command("dev")
    .description(
        "Plugin development commands (use 'alcedo dev proxy' to start the dev proxy)",
    )
    .addCommand(proxyCommand)
    .addCommand(buildFrontendCommand)
    .addCommand(serveFrontendCommand);
