import { cosmiconfigSync } from "cosmiconfig";
import path from "node:path";
import fs from "node:fs";

export interface ConfigSchema {
  registryUrl?: string;
  coreUrl?: string;
  pluginDir?: string;
  apiKey?: string;
}

const DEFAULTS: ConfigSchema = {
  registryUrl: "localhost:5000",
  coreUrl: "http://localhost:8080",
  pluginDir: process.cwd(),
};

const ENV_PREFIX = "ALCEDO_";

/**
 * Load config with priority: CLI flags highest → env vars → .alcedorc → defaults lowest.
 * `cliFlags` is the parsed Commander.js flags object (already processed by Commander).
 */
export function loadConfig(cliFlags: Partial<ConfigSchema> = {}): ConfigSchema {
  // Start with defaults
  const config: ConfigSchema = { ...DEFAULTS };

  // Override with .alcedorc (lowest priority override)
  const rcConfig = loadAlcedorc();
  if (rcConfig) {
    Object.assign(config, rcConfig);
  }

  // Override with .alcedocore.dev.env (set by `alcedocore connect`)
  const devEnvConfig = loadDevEnv();
  Object.assign(config, devEnvConfig);

  // Override with env vars (medium priority)
  const envConfig = loadEnvConfig();
  Object.assign(config, envConfig);

  // Override with CLI flags (highest priority)
  Object.assign(config, cliFlags);

  return config;
}

/**
 * Walk up directory tree from startDir (default: cwd) to find .alcedorc JSON file.
 * Returns the parsed config or null if no file found.
 * Per CLI-06: auto-detection walks up directory tree.
 */
function loadAlcedorc(): ConfigSchema | null {
  const explorer = cosmiconfigSync("alcedo", {
    searchPlaces: [".alcedorc", ".alcedorc.json"],
    stopDir: path.parse(process.cwd()).root,
  });
  
  // cosmiconfig's search walks up automatically
  const result = explorer.search();
  if (result && !result.isEmpty) {
    return result.config as ConfigSchema;
  }
  return null;
}

/**
 * Read .alcedocore.dev.env from the current directory (set by `alcedocore connect`).
 * Format: KEY=VALUE (one per line)
 */
function loadDevEnv(): Partial<ConfigSchema> {
  const config: Partial<ConfigSchema> = {};
  const envPath = path.join(process.cwd(), ".alcedocore.dev.env");
  if (!fs.existsSync(envPath)) return config;

  const content = fs.readFileSync(envPath, "utf-8");
  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eqIdx = trimmed.indexOf("=");
    if (eqIdx === -1) continue;
    const key = trimmed.slice(0, eqIdx).trim();
    const value = trimmed.slice(eqIdx + 1).trim();
    if (!value) continue;
    if (key === "CORE_URL") config.coreUrl = value;
    if (key === "API_KEY") config.apiKey = value;
  }
  return config;
}

/**
 * Read environment variables with ALCEDO_ prefix.
 * e.g., ALCEDO_REGISTRY_URL → registryUrl, ALCEDO_CORE_URL → coreUrl
 */
function loadEnvConfig(): Partial<ConfigSchema> {
  const config: Partial<ConfigSchema> = {};
  
  const envMapping: Record<string, keyof ConfigSchema> = {
    ALCEDO_REGISTRY_URL: "registryUrl",
    ALCEDO_CORE_URL: "coreUrl",
    ALCEDO_PLUGIN_DIR: "pluginDir",
    ALCEDO_API_KEY: "apiKey",
  };

  for (const [envKey, configKey] of Object.entries(envMapping)) {
    const value = process.env[envKey];
    if (value) {
      (config as any)[configKey] = value;
    }
  }

  return config;
}

/**
 * Explicit directory tree walk for .alcedorc discovery (CLI-06).
 * Used when cosmiconfig's auto-search isn't desired and we need
 * manual control over the walk.
 */
export function findAlcedorc(startDir: string = process.cwd()): string | null {
  let currentDir = path.resolve(startDir);
  
  while (true) {
    const candidate = path.join(currentDir, ".alcedorc");
    if (fs.existsSync(candidate)) {
      return candidate;
    }
    
    const parent = path.dirname(currentDir);
    if (parent === currentDir) {
      // Reached filesystem root
      return null;
    }
    currentDir = parent;
  }
}
