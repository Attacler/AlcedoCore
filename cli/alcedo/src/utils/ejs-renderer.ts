import ejs from "ejs";
import fs from "node:fs";
import path from "node:path";
import { error as logError } from "./logger.js";

/**
 * Render an EJS template string with the given data.
 * Returns the rendered string.
 */
export function renderTemplate(
  templateContent: string,
  data: Record<string, any>
): string {
  return ejs.render(templateContent, data);
}

/**
 * Read an EJS template file, render it with data, and write to the target path.
 * Creates parent directories if they don't exist.
 * ONLY creates new files — never overwrites (GEN-05 compliance).
 * Returns the target path on success, throws on error.
 */
export function renderAndWrite(
  templatePath: string,
  targetPath: string,
  data: Record<string, any>
): string {
  if (fs.existsSync(targetPath)) {
    throw new Error(
      `Target file already exists: ${targetPath}. ` +
      `Refusing to overwrite existing code (GEN-05).`
    );
  }

  const templateContent = fs.readFileSync(templatePath, "utf-8");
  const rendered = renderTemplate(templateContent, data);

  const dir = path.dirname(targetPath);
  fs.mkdirSync(dir, { recursive: true });

  fs.writeFileSync(targetPath, rendered, "utf-8");
  return targetPath;
}

/**
 * Read an EJS template file and return the rendered content as a string
 * (without writing to disk). Used for content that gets appended to existing files
 * like manifest.json updates.
 */
export function renderToString(
  templatePath: string,
  data: Record<string, any>
): string {
  const templateContent = fs.readFileSync(templatePath, "utf-8");
  return renderTemplate(templateContent, data);
}
