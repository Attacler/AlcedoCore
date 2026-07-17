import { Command } from "commander";
import { error as logError } from "../utils/logger.js";
import { addMigrationAction } from "./add-migration.js";

const generateCommand = new Command("generate")
  .argument("<name>", "Migration name (e.g., create_users_table)")
  .description("Alias for `alcedo add migration` — generate a versioned SQL migration pair")
  .action(async (name: string) => {
    try {
      await addMigrationAction(name);
    } catch (err: any) {
      logError(`Generate failed: ${err.message}`);
      process.exit(1);
    }
  });

export const migrateCommand = new Command("migrate")
  .description("Alias for `alcedo add migration`");

migrateCommand.addCommand(generateCommand);
