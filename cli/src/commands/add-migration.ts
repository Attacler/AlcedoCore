import { Command } from "commander";
import path from "node:path";
import fs from "node:fs";
import {
    createSpinner,
    success,
    error as logError,
    info,
} from "../utils/logger.js";
import { renderAndWrite } from "../utils/ejs-renderer.js";
import { loadConfig } from "../config.js";
import { assertSafeName } from "../utils/validation.js";
import { generateTimestamp } from "../utils/generateTimestamp.js";

/**
 * Derive a table name from migration name.
 * Converts "create_users_table" → "users", "add_category_to_items" → "category"
 * Falls back to a safe default if pattern doesn't match.
 */
function deriveTableName(migrationName: string): string {
    // Pattern: create_{table_name}_table -> table_name
    const createMatch = migrationName.match(/^create_(.+)_table$/);
    if (createMatch) return createMatch[1];

    // Pattern: add_{column}_to_{table} -> table
    const addMatch = migrationName.match(/^add_\w+_to_(.+)$/);
    if (addMatch) return addMatch[1];

    // Fallback: use the migration name itself (already validated as safe by assertSafeName)
    return migrationName;
}

/**
 * Shared migration generation action. Creates a versioned SQL migration file pair
 * (.up.sql + .down.sql) in the plugin's migrations/ directory.
 * Can be used by both `alcedo add migration` and `alcedo migrate generate`.
 */
export async function addMigrationAction(name: string): Promise<void> {
    const config = loadConfig();

        assertSafeName(name, "migration name");

    const pluginDir = config.pluginDir || process.cwd();
    const migrationsDir = path.resolve(pluginDir, "migrations");

    if (!fs.existsSync(migrationsDir)) {
        throw new Error(
            `No migrations/ directory found in ${pluginDir}. Run \`alcedo init\` first or create the directory.`,
        );
    }

    const timestamp = generateTimestamp();
    const tableName = deriveTableName(name);
    const templatesDir = path.resolve(__dirname, "../../templates");

    const data = {
        name,
        timestamp,
        tableName,
    };

    const upFilename = `${timestamp}_${name}.up.sql`;
    const downFilename = `${timestamp}_${name}.down.sql`;

    const spinner = createSpinner(`Generating migration: ${name}`);

    try {
        // Generate up migration
        renderAndWrite(
            path.join(templatesDir, "migration", "up.sql.ejs"),
            path.join(migrationsDir, upFilename),
            data,
        );

        // Generate down migration
        renderAndWrite(
            path.join(templatesDir, "migration", "down.sql.ejs"),
            path.join(migrationsDir, downFilename),
            data,
        );

        spinner.succeed();

        success(`Migration created:`);
        info(`  ${upFilename}`);
        info(`  ${downFilename}`);
        info(`Edit these files to add your SQL logic.`);
    } catch (err: any) {
        spinner.fail();
        logError(`Failed to generate migration: ${err.message}`);
        process.exit(1);
    }
}

export const addMigrationCommand = new Command("migration")
    .argument("<name>", "Migration name (e.g., create_users_table)")
    .description(
        "Generate a versioned SQL migration file pair (.up.sql + .down.sql)",
    )
    .action(async (name: string) => {
        await addMigrationAction(name);
    });
