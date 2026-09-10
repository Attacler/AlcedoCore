import { Command } from "commander";
import { createSpinner, success, error as logError, info } from "../utils/logger";
import { loadConfig } from "../config";

interface DeployOptions {
  tag?: string;
  image?: string;
  start?: boolean;
  env?: string[];
}

function parseEnvVars(env: string[] = []): Record<string, string> {
  const result: Record<string, string> = {};
  for (const e of env) {
    const eqIdx = e.indexOf("=");
    if (eqIdx === -1) {
      result[e] = "";
    } else {
      result[e.slice(0, eqIdx)] = e.slice(eqIdx + 1);
    }
  }
  return result;
}

interface RegistryItem {
  id: number;
  name: string;
  url: string;
}

/**
 * Resolve the registry_id for a deploy from the core's registry list.
 *
 * Deploys ALWAYS pull from a configured registry on the core, so we match the
 * local registry URL against the core's registry entries and fall back to the
 * first/default one when nothing matches.
 */
async function resolveRegistryId(
  coreUrl: string,
  apiKey: string | undefined,
  registryUrl: string,
): Promise<number> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (apiKey) {
    headers["Authorization"] = `Bearer ${apiKey}`;
  }

  const res = await fetch(`${coreUrl.replace(/\/$/, "")}/api/registries`, { headers });
  if (!res.ok) {
    throw new Error(`Failed to list registries (${res.status})`);
  }
  const json: any = await res.json();
  const registries: RegistryItem[] = json?.data?.registries || json?.registries || [];
  if (!registries.length) {
    throw new Error(
      "No registries configured on the core. Seed one via REGISTRY_URL/REGISTRY_NAME or the admin API, then retry.",
    );
  }

  const normalized = (url: string) =>
    (url || "").replace(/^https?:\/\//, "").replace(/\/$/, "");
  const target = normalized(registryUrl);
  const match = registries.find((r) => normalized(r.url) === target);
  return (match || registries[0]).id;
}

export const deployCommand = new Command("deploy")
  .argument("<slug>", "Plugin slug (e.g., my-plugin)")
  .option("-t, --tag <version>", "Plugin version tag", "1.0.0")
  .option("-i, --image <image>", "Docker image (default: {registry}/{slug}:{tag})")
  .option("--no-start", "Register only, don't start the container")
  .option("-e, --env <key=value>", "Environment variables (repeatable)", collectEnv, [])
  .description("Deploy a plugin to Alcedo Core")
  .action(async (slug: string, options: DeployOptions, cmd: Command) => {
    const config = loadConfig(cmd.optsWithGlobals() as any);
    const registryUrl = (config.registryUrl || "localhost:5000").replace(/^https?:\/\//, "");
    const coreUrl = config.coreUrl;
    const tag = options.tag || "1.0.0";
    const image = options.image || `${registryUrl}/${slug}:${tag}`;

    const spinner = createSpinner(`Deploying plugin: ${slug} v${tag}`);

    try {
      const registryId = await resolveRegistryId(
        coreUrl,
        config.apiKey || process.env.ALCEDO_API_KEY,
        registryUrl,
      );

      const body = JSON.stringify({
        slug,
        version: tag,
        image,
        registry_id: registryId,
        env: parseEnvVars(options.env),
        start_container: options.start !== false,
      });

      const headers: Record<string, string> = { "Content-Type": "application/json" };
      const apiKey = config.apiKey || process.env.ALCEDO_API_KEY;
      if (apiKey) {
        headers["Authorization"] = `Bearer ${apiKey}`;
      }

      const res = await fetch(`${coreUrl}/api/plugins/deploy`, {
        method: "POST",
        headers,
        body,
      });

      if (!res.ok) {
        const errBody = await res.text().catch(() => "Unknown error");
        throw new Error(`Deploy failed (${res.status}): ${errBody}`);
      }

      const contentType = res.headers.get("content-type") || "";
      if (!contentType.includes("json")) {
        const text = await res.text();
        throw new Error(`Expected JSON but got ${contentType}: ${text.slice(0, 200)}`);
      }

      const result: any = await res.json();
      const data = result.data || result;

      spinner.succeed();
      success(`Plugin "${slug}" v${tag} deployed`);
      if (data.container_id) {
        info(`Container: ${data.container_id}`);
      }
    } catch (err: any) {
      spinner.fail();
      logError(`Failed to deploy: ${err.message}`);
      process.exit(1);
    }
  });

function collectEnv(val: string, prev: string[]): string[] {
  prev.push(val);
  return prev;
}
