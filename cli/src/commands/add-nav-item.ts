import { Command } from "commander";
import path from "node:path";
import fs from "node:fs";
import {
    createSpinner,
    success,
    error as logError,
    info,
} from "../utils/logger.js";
import { loadConfig, ConfigSchema } from "../config.js";
import { assertSafeName } from "../utils/validation.js";
import { toPascalCase } from "../utils/formatting.js";

export const addNavItemCommand = new Command("nav-item")
    .argument("<name>", "Nav item name (e.g., settings, user-management)")
    .option("-p, --path <path>", "Destination URL path (default: /{name})")
    .option("-i, --icon <icon>", "Material Symbol icon name (default: page)")
    .option("--parent <parent>", "Parent nav-item path for nesting")
    .option("-b, --badge <badge>", "Badge text for the nav item")
    .description(
        "Add a sidebar navigation entry to manifest.json (no file created)",
    )
    .action(
        async (
            name: string,
            options: {
                path?: string;
                icon?: string;
                parent?: string;
                badge?: string;
            },
            cmd: Command,
        ) => {
            const config = loadConfig(
                cmd.optsWithGlobals() as Partial<ConfigSchema>,
            );

            assertSafeName(name, "nav-item name");

            const pluginDir = config.pluginDir || process.cwd();
            const manifestPath = path.resolve(pluginDir, "manifest.json");

            // Derive nav-item metadata
            const label = toPascalCase(name);
            const targetPath = options.path || "/" + name;
            const icon = options.icon || "page";
            const parent = options.parent;
            const badge = options.badge;

            const spinner = createSpinner(`Adding nav-item: ${name}`);

            try {
                // Nav-item is manifest-only — no file created
                if (!fs.existsSync(manifestPath)) {
                    throw new Error(
                        `No manifest.json found at ${manifestPath} — skipping.`,
                    );
                }
                const manifest = JSON.parse(
                    fs.readFileSync(manifestPath, "utf-8"),
                );

                // Initialize pages array if it doesn't exist
                if (!manifest.pages) {
                    manifest.pages = [];
                }

                // Check for duplicate before appending
                const isDuplicate = manifest.pages.some(
                    (e: any) => e.path === targetPath,
                );

                if (isDuplicate) {
                    throw new Error(
                        `Nav-item at ${targetPath} already exists in manifest.json — skipping.`,
                    );
                }
                // Construct nav-entry as a plain JS object (avoids JSON injection via EJS)
                const navEntry: Record<string, any> = {
                    label,
                    path: targetPath,
                    icon,
                    sidebar: true,
                };
                if (parent) navEntry.parent = parent;
                if (badge) navEntry.badge = badge;
                manifest.pages.push(navEntry);

                // Write updated manifest.json back (preserving existing data)
                fs.writeFileSync(
                    manifestPath,
                    JSON.stringify(manifest, null, 2) + "\n",
                    "utf-8",
                );

                spinner.succeed();

                success(`Nav-item added:`);
                info(`  manifest.json ← ${label} (${targetPath})`);
            } catch (err: any) {
                spinner.fail();
                logError(`Failed to add nav-item: ${err.message}`);
                process.exit(1);
            }
        },
    );
