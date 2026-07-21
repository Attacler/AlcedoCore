import { Command } from "commander";
import path from "node:path";
import fs from "node:fs";
import {
    createSpinner,
    success,
    error as logError,
    info,
} from "../utils/logger";
import { renderAndWrite } from "../utils/ejs-renderer";
import { loadConfig, ConfigSchema } from "../config";
import { assertSafeName } from "../utils/validation";
import { toPascalCase } from "../utils/formatting";

export const addPageCommand = new Command("page")
    .argument("<name>", "Page name (e.g., dashboard)")
    .option("-r, --route <path>", "URL path (e.g., /my-page)")
    .option("-i, --icon <icon>", "Material Symbol icon name")
    .description("Generate a Vue 3 page component and update manifest.json")
    .action(
        async (
            name: string,
            options: { route?: string; icon?: string },
            cmd: Command,
        ) => {
            const config = loadConfig(
                cmd.optsWithGlobals() as Partial<ConfigSchema>,
            );

            assertSafeName(name, "page name");

            const pluginDir = config.pluginDir || process.cwd();
            const manifestPath = path.resolve(pluginDir, "manifest.json");

            // Derive page metadata
            const label = toPascalCase(name);
            const route = options.route || "/" + name;
            const icon = options.icon || "page";

            const templatesDir = path.resolve(__dirname, "../../templates");
            const pagesDir = path.resolve(pluginDir, "pages");

            const spinner = createSpinner(`Generating page: ${name}`);

            try {
                if (!fs.existsSync(manifestPath)) {
                    throw new Error(
                        `No manifest.json found at ${manifestPath} — skipping manifest update.`,
                    );
                }
                renderAndWrite(
                    path.join(templatesDir, "page", "page.vue.ejs"),
                    path.join(pagesDir, `${name}.vue`),
                    { name, label, route, icon },
                );
                const manifest = JSON.parse(
                    fs.readFileSync(manifestPath, "utf-8"),
                );

                // Initialize pages array if it doesn't exist
                if (!manifest.pages) {
                    manifest.pages = [];
                }

                // Check for duplicate before appending (GEN-05)
                const isDuplicate = manifest.pages.some(
                    (e: any) => e.path === route,
                );

                if (isDuplicate) {
                    spinner.succeed();
                    info(
                        `Page ${route} already exists in manifest.json — skipping.`,
                    );
                    return;
                }
                manifest.pages.push({ label, path: route, sidebar: true });

                // Write updated manifest.json back (preserving existing data)
                fs.writeFileSync(
                    manifestPath,
                    JSON.stringify(manifest, null, 2) + "\n",
                    "utf-8",
                );

                spinner.succeed();

                success(`Page created:`);
                info(`  pages/${name}.vue`);
                info(`  manifest.json ← ${label} (${route})`);
                info(
                    `Register this page in pages/main.ts by importing and adding to the pages array.`,
                );
            } catch (err: any) {
                spinner.fail();
                logError(`Failed to generate page: ${err.message}`);
                process.exit(1);
            }
        },
    );
