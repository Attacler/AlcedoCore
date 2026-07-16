import { Command } from "commander";

export const devCommand = new Command("dev")
  .description("Plugin development commands (use 'alcedo dev proxy' to start the dev proxy)");
