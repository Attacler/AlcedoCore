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
import readline from "node:readline";

/**
 * Prompt user for language selection (Python or Node.js).
 * Uses readline for stdin prompt.
 */
function promptLanguage(): Promise<"python" | "node"> {
    const rl = readline.createInterface({
        input: process.stdin,
        output: process.stdout,
    });

    return new Promise((resolve) => {
        rl.question("Select plugin language (python/node): ", (answer) => {
            rl.close();
            const lang = answer.trim().toLowerCase();
            if (lang === "python" || lang === "py") {
                resolve("python");
            } else if (
                lang === "node" ||
                lang === "nodejs" ||
                lang === "javascript" ||
                lang === "js"
            ) {
                resolve("node");
            } else {
                // Default to python on invalid input, matching hello-world
                info(`Unknown language "${lang}", defaulting to Python`);
                resolve("python");
            }
        });
    });
}

function slugify(name: string): string {
    return name
        .toLowerCase()
        .replace(/[^a-z0-9-]/g, "-")
        .replace(/-+/g, "-")
        .replace(/^-|-$/g, "");
}

function copyGitkeep(
    templateDir: string,
    targetDir: string,
    subDir: string,
): void {
    const src = path.join(templateDir, "plugin", subDir, ".gitkeep");
    const dest = path.join(targetDir, subDir, ".gitkeep");
    if (fs.existsSync(src)) {
        fs.mkdirSync(path.dirname(dest), { recursive: true });
        fs.writeFileSync(dest, "");
    }
}

function parseLanguage(lang: string): "python" | "node" {
    const v = lang.trim().toLowerCase();
    if (v === "python" || v === "py") return "python";
    if (v === "node" || v === "nodejs" || v === "javascript" || v === "js")
        return "node";
    throw new Error(`Unknown language "${lang}". Use "python" or "node".`);
}

export const initCommand = new Command("init")
    .argument("<name>", "Plugin project name (e.g., my-plugin)")
    .option("-l, --language <language>", "Plugin language (python or node)")
    .description("Scaffold a new plugin project")
    .action(
        async (name: string, options: { language?: string }, cmd: Command) => {
            const config = loadConfig(cmd.optsWithGlobals() as any);
            const pluginDir = config.pluginDir || process.cwd();
            const targetDir = path.resolve(pluginDir, name);
            const slug = slugify(name);

            if (fs.existsSync(targetDir)) {
                logError(`Directory already exists: ${targetDir}`);
                process.exit(1);
            }

            const language = options.language
                ? parseLanguage(options.language)
                : await promptLanguage();

            const templatesDir = path.resolve(__dirname, "../../templates");

            const registryUrl = config.registryUrl || "localhost:5000";
            const data = {
                name,
                slug,
                version: "1.0.0",
                description: `A new Alcedo plugin`,
                language,
                registryUrl,
            };

            const spinner = createSpinner(`Scaffolding plugin: ${name}`);

            try {
                // Create target directory
                fs.mkdirSync(targetDir, { recursive: true });

                // Generate files from templates
                // manifest.json
                renderAndWrite(
                    path.join(templatesDir, "plugin", "manifest.json.ejs"),
                    path.join(targetDir, "manifest.json"),
                    data,
                );

                // Dockerfile (language-specific)
                const dockerTemplate =
                    language === "python"
                        ? "Dockerfile.ejs"
                        : "Dockerfile.node.ejs";
                renderAndWrite(
                    path.join(templatesDir, "plugin", dockerTemplate),
                    path.join(targetDir, "Dockerfile"),
                    data,
                );

                // Server stub (language-specific)
                const serverTemplate =
                    language === "python" ? "server.py.ejs" : "server.js.ejs";
                renderAndWrite(
                    path.join(templatesDir, "plugin", serverTemplate),
                    path.join(
                        targetDir,
                        `server.${language === "python" ? "py" : "js"}`,
                    ),
                    data,
                );

                // .gitignore
                renderAndWrite(
                    path.join(templatesDir, "plugin", "gitignore.ejs"),
                    path.join(targetDir, ".gitignore"),
                    data,
                );

                // README.md
                renderAndWrite(
                    path.join(templatesDir, "plugin", "README.md.ejs"),
                    path.join(targetDir, "README.md"),
                    data,
                );

                // Empty directories with .gitkeep
                copyGitkeep(templatesDir, targetDir, "migrations");
                copyGitkeep(templatesDir, targetDir, "pages");
                copyGitkeep(templatesDir, targetDir, "public");

                spinner.succeed();

                success(`Plugin scaffolded: ${targetDir}`);
                info(`Next steps:
      cd ${name}
      # Edit server.${language === "python" ? "py" : "js"} and manifest.json
      # Build with: docker build -t ${registryUrl}/${slug}:1.0.0 .`);
            } catch (err: any) {
                spinner.fail();
                logError(`Failed to scaffold plugin: ${err.message}`);
                process.exit(1);
            }
        },
    );
