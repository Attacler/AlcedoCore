import ky, { type BeforeRequestHook } from "ky";
import { createPluginsResource } from "./plugins.js";
import { createHealthResource } from "./health.js";
import { createMigrationsResource } from "./migrations.js";
import { createSettingsResource } from "./settings.js";
import { createUsageResource } from "./usage.js";
import { createKvResource } from "./kv.js";
import { createDbResource } from "./db.js";
import { createSchemaResource } from "./schema.js";
import { createLogsResource } from "./logs.js";
import { createDevResource } from "./dev.js";
import { createAuthResource } from "./auth.js";
import { createUsersResource } from "./users.js";
import { createRolesResource } from "./roles.js";
import { createPoliciesResource } from "./policies.js";
import { createCollectionsResource } from "./collections.js";
import { createItemsResource } from "./items.js";
import { createFilesResource } from "./files.js";
import { createRegistriesResource } from "./registries.js";
import { createActivityLogsResource } from "./activityLogs.js";
import { createAppSettingsResource } from "./appSettings.js";
import { createDeveloperApiKeysResource } from "./developerApiKeys.js";

function generateUUID(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

export class AlcedoError extends Error {
  status: number;
  statusText: string;
  code?: string;

  constructor(status: number, statusText: string, message: string, code?: string) {
    super(message);
    this.status = status;
    this.statusText = statusText;
    this.message = message;
    this.code = code;
    this.name = "AlcedoError";
  }
}

export class ConnectionError extends AlcedoError {
  original?: Error;

  constructor(message: string, status: number = 0, original?: Error) {
    super(status, "Connection Error", message);
    this.name = "ConnectionError";
    this.original = original;
  }
}

export class NotFoundError extends AlcedoError {
  key?: string;

  constructor(message: string, key?: string) {
    super(404, "Not Found", message);
    this.name = "NotFoundError";
    this.key = key;
  }
}

export class ValidationError extends AlcedoError {
  details?: any;

  constructor(message: string, status: number = 400, details?: any) {
    super(status, "Validation Error", message);
    this.name = "ValidationError";
    this.details = details;
  }
}

export class AuthenticationError extends AlcedoError {
  constructor(message: string) {
    super(401, "Authentication Error", message);
    this.name = "AuthenticationError";
  }
}

export class ServerError extends AlcedoError {
  constructor(message: string, status: number = 500) {
    super(status, "Server Error", message);
    this.name = "ServerError";
  }
}

const DEFAULT_TIMEOUT = 30_000;

export interface ClientOptions {
  timeout?: number;
  requestId?: string;
  retry?: {
    limit?: number;
    delay?: (attempt: number) => number;
  };
}

export function createClient(baseUrl: string, options: ClientOptions = {}) {
  const kyInstance = ky.create({
    baseUrl: baseUrl.replace(/\/$/, ""),
    prefix: "/api",
    timeout: options.timeout ?? DEFAULT_TIMEOUT,
    retry: {
      limit: options.retry?.limit ?? 3,
      delay: options.retry?.delay ?? ((attempt: number) => Math.pow(2, attempt) * 1000),
      methods: ["get", "post", "put", "delete", "patch"] as any,
      statusCodes: [408, 413, 429, 500, 502, 503, 504],
    },
    hooks: {
      beforeRequest: [
        ((state: any) => {
          state.request.headers.set("X-Request-ID", state.options?.requestId ?? generateUUID());
          state.request.headers.set("X-Plugin-Slug", state.options?.pluginSlug ?? "system");
        }) as BeforeRequestHook,
      ],
    },
  });

  function createRequest(ky2: typeof kyInstance) {
    return async (method: string, path: string, opts?: any) => {
      return (ky2 as any)[method](path, opts);
    };
  }

  return {
    plugins: createPluginsResource(kyInstance),
    health: createHealthResource(kyInstance),
    activityLogs: createActivityLogsResource(kyInstance),
    appSettings: createAppSettingsResource(kyInstance),
    developerApiKeys: createDeveloperApiKeysResource(kyInstance),
    registries: createRegistriesResource(kyInstance),
    migrations: createMigrationsResource(kyInstance),
    settings: createSettingsResource(kyInstance),
    usage: createUsageResource(kyInstance),
    kv: createKvResource(kyInstance),
    db: createDbResource(kyInstance),
    schema: createSchemaResource(kyInstance),
    logs: createLogsResource(kyInstance),
    dev: createDevResource(kyInstance),
    auth: createAuthResource(kyInstance),
    users: createUsersResource(kyInstance),
    roles: createRolesResource(kyInstance),
    policies: createPoliciesResource(kyInstance),
    collections: createCollectionsResource(kyInstance),
    items: createItemsResource(kyInstance),
    files: createFilesResource(kyInstance),
    request: createRequest(kyInstance),
  };
}
