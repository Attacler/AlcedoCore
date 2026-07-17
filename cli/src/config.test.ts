import { describe, it, expect, afterEach } from "vitest";
import { loadConfig, findAlcedorc } from "./config.js";
import os from "node:os";

describe("loadConfig", () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it("returns defaults when no config sources are present", () => {
    const config = loadConfig({});
    expect(config.registryUrl).toBe("localhost:5000");
    expect(config.coreUrl).toBe("http://localhost:8080");
    expect(config.pluginDir).toBe(process.cwd());
  });

  it("prefers CLI flags over env vars", () => {
    process.env.ALCEDO_CORE_URL = "http://env-url:8080";
    const config = loadConfig({ coreUrl: "http://cli-url:8080" });
    expect(config.coreUrl).toBe("http://cli-url:8080");
  });

  it("prefers env vars over defaults", () => {
    process.env.ALCEDO_CORE_URL = "http://env-url:8080";
    const config = loadConfig({});
    expect(config.coreUrl).toBe("http://env-url:8080");
  });

  it("reads ALCEDO_REGISTRY_URL from env", () => {
    process.env.ALCEDO_REGISTRY_URL = "my-registry:5000";
    const config = loadConfig({});
    expect(config.registryUrl).toBe("my-registry:5000");
  });

  it("reads ALCEDO_PLUGIN_DIR from env", () => {
    process.env.ALCEDO_PLUGIN_DIR = "/custom/plugin/path";
    const config = loadConfig({});
    expect(config.pluginDir).toBe("/custom/plugin/path");
  });
});

describe("findAlcedorc", () => {
  it("returns null when no .alcedorc file exists", () => {
    // Use system temp dir where no .alcedorc should exist
    const result = findAlcedorc(os.tmpdir());
    expect(result).toBeNull();
  });
});
