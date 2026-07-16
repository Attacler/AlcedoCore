export {
  AlcedoError,
  ConnectionError,
  NotFoundError,
  ValidationError,
  AuthenticationError,
  ServerError,
  createClient,
} from "./client.js";
export type { ClientOptions } from "./client.js";

export { createHealthResource } from "./health.js";
export { createPluginsResource } from "./plugins.js";
export { createMigrationsResource } from "./migrations.js";
export { createSettingsResource } from "./settings.js";
export { createUsageResource } from "./usage.js";
export { createKvResource } from "./kv.js";
export { createDbResource } from "./db.js";
export { createSchemaResource } from "./schema.js";
export { createLogsResource } from "./logs.js";
export { createDevResource } from "./dev.js";
export { createAuthResource } from "./auth.js";
export { createUsersResource } from "./users.js";
export { createRolesResource } from "./roles.js";
export { createPoliciesResource } from "./policies.js";
export { createCollectionsResource } from "./collections.js";
export { createItemsResource } from "./items.js";
export { createFilesResource } from "./files.js";
export { createRegistriesResource } from "./registries.js";
export { createActivityLogsResource } from "./activityLogs.js";
export { createAppSettingsResource } from "./appSettings.js";
export { createDeveloperApiKeysResource } from "./developerApiKeys.js";

export {
  PluginSchema,
  PluginListSchema,
  PluginSchemaResponseSchema,
  MigrationStatusSchema,
  MigrationListSchema,
  SettingsResponseSchema,
  WriteSuccessResponseSchema,
  HealthResponseSchema,
  PluginPageSchema,
  PluginPagesResponseSchema,
  PluginAssetsResponseSchema,
} from "./zod-schemas.js";

export type {
  Plugin,
  PluginSchemaResponse,
  MigrationStatus,
  SettingsResponse,
  HealthResponse,
  PluginPage,
  PluginPagesResponse,
  PluginAssetsResponse,
  WriteSuccessResponse,
} from "./zod-schemas.js";
