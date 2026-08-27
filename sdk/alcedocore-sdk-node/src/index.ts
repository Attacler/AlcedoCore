export { createClient } from "./client";
export type { ClientOptions } from "./client";

export { createHealthResource } from "./health";
export { createPluginsResource } from "./plugins";
export { createMigrationsResource } from "./migrations";
export { createSettingsResource } from "./settings";
export { createUsageResource } from "./usage";
export { createKvResource } from "./kv";
export { createDbResource } from "./db";
export { createSchemaResource } from "./schema";
export { createLogsResource } from "./logs";
export { createDevResource } from "./dev";
export { createAuthResource } from "./auth";
export { createUsersResource } from "./users";
export { createRolesResource } from "./roles";
export { createPoliciesResource } from "./policies";
export { createCollectionsResource } from "./collections";
export { createItemsResource } from "./items";
export { createFilesResource } from "./files";
export { createRegistriesResource } from "./registries";
export { createActivityLogsResource } from "./activityLogs";
export { createAppSettingsResource } from "./appSettings";
export { createDeveloperApiKeysResource } from "./developerApiKeys";

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
} from "./zod-schemas";

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
} from "./zod-schemas";

export type { DeveloperKey } from "./types/developerKeys";
export type { FileFolder, MediaFile, ListFilesParameters } from "./types/files";
