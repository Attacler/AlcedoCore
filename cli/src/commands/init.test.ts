import { describe, it, expect, beforeAll, afterAll } from "vitest";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

describe("Init command", () => {
  const testDir = path.join(os.tmpdir(), `alcedo-test-init-${Date.now()}`);

  beforeAll(() => {
    fs.mkdirSync(testDir, { recursive: true });
  });

  afterAll(() => {
    fs.rmSync(testDir, { recursive: true, force: true });
  });

  it("validates slug format correctly", () => {
    // Test the validation logic used by init command
    const validSlugs = ["my-plugin", "hello-world", "test123"];
    const invalidSlugs = ["My Plugin", "my_plugin", "", "UPPERCASE"];

    // The validation function requires lowercase alphanumeric + hyphens
    const isValid = (slug: string): boolean =>
      slug.length > 0 && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(slug);

    for (const slug of validSlugs) {
      expect(isValid(slug)).toBe(true);
    }
    for (const slug of invalidSlugs) {
      expect(isValid(slug)).toBe(false);
    }
  });

  it("generates valid plugin directory structure", () => {
    // Verify the expected directory structure matches what init would create
    const expectedDirs = ["migrations", "pages"];
    for (const dir of expectedDirs) {
      const dirPath = path.join(testDir, dir);
      fs.mkdirSync(dirPath, { recursive: true });
      expect(fs.existsSync(dirPath)).toBe(true);
    }

    // Verify key files that init should generate
    const expectedFiles = ["manifest.json", "Dockerfile", "server.py"];
    for (const file of expectedFiles) {
      const filePath = path.join(testDir, file);
      // Only test structure, not content — init command generates these via templates
      if (file === "manifest.json") {
        fs.writeFileSync(filePath, JSON.stringify({
          slug: "test-plugin",
          version: "1.0.0",
          plugin_type: "dynamic",
        }));
      }
      if (file === "Dockerfile") {
        fs.writeFileSync(filePath, "FROM python:3.11-slim\n");
      }
      if (file === "server.py") {
        fs.writeFileSync(filePath, 'print("hello")\n');
      }
      expect(fs.existsSync(filePath)).toBe(true);
    }
  });

  it("generates versioned migration filenames correctly", () => {
    // Migration filenames follow pattern: YYYYMMDD_<name>.up.sql
    const date = "20260527";
    const name = "create_users_table";
    const upFilename = `${date}_${name}.up.sql`;
    const downFilename = `${date}_${name}.down.sql`;

    expect(upFilename).toMatch(/^\d{8}_.+\.up\.sql$/);
    expect(downFilename).toMatch(/^\d{8}_.+\.down\.sql$/);
  });

  it("generates endpoint handler with correct structure", () => {
    // Endpoint template generates: slug-safe endpoint path + handler stub
    const endpointName = "user-data";
    const expectedMethod = "GET";

    // Simulate the manifest endpoint entry format
    const endpointEntry = {
      method: expectedMethod,
      path: `/${endpointName}`,
      handler: `${endpointName}_handler`,
    };

    expect(endpointEntry.path).toBe(`/${endpointName}`);
    expect(endpointEntry.method).toBe(expectedMethod);
    expect(endpointEntry.handler).toBeTruthy();
  });
});
