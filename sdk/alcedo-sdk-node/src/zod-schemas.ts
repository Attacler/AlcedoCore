import { z } from "zod";

export const PluginSchema = z.object({
  name: z.string(),
  version: z.string(),
  status: z.enum(["enabled", "disabled", "error"]),
  plugin_type: z.string().optional(),
  type: z.string().optional(),
});

export const PluginListSchema = z.array(PluginSchema);

const ColumnSchema = z.object({
  name: z.string(),
  type: z.string(),
  nullable: z.boolean(),
  default: z.string().nullable().optional(),
})

const PrimaryKeySchema = z.object({
  constraintName: z.string(),
  columns: z.array(z.string()),
})

const ForeignKeySchema = z.object({
  constraintName: z.string(),
  columnName: z.string(),
  foreignTableName: z.string(),
  foreignColumnName: z.string(),
})

const TableSchema = z.object({
  name: z.string(),
  columns: z.array(ColumnSchema),
  primaryKey: PrimaryKeySchema.optional(),
  foreignKeys: z.array(ForeignKeySchema),
})

export const PluginSchemaResponseSchema = z.object({
  tables: z.array(TableSchema).optional(),
})

export const MigrationStatusSchema = z.object({
  name: z.string(),
  version: z.string().optional(),
  sql: z.string().optional(),
  appliedAt: z.string().nullish(),
  pending: z.boolean(),
});

export const MigrationListSchema = z.array(MigrationStatusSchema);

export const SettingsResponseSchema = z.record(z.string(), z.unknown());

export const WriteSuccessResponseSchema = z.object({
  success: z.literal(true),
  message: z.string(),
});

export const HealthResponseSchema = z.object({
  status: z.string(),
  version: z.string().optional(),
});

export const PluginPageSchema = z.object({
  path: z.string(),
  label: z.string(),
  icon: z.string().optional(),
  sidebar: z.boolean().optional(),
});

export const PluginPagesResponseSchema = z.object({
  pages: z.array(PluginPageSchema),
});

export const PluginAssetsResponseSchema = z.object({
  js: z.string(),
  css: z.string(),
});

export const PluginManifestSchema = z.object({
  manifestVersion: z.number().int().positive(),
  pluginSlug: z.string().min(1),
  pages: z.array(z.object({
    path: z.string(),
    label: z.string(),
    icon: z.string().optional(),
    sidebar: z.boolean().optional(),
  })).optional(),
  views: z.array(z.object({
    path: z.string(),
    component: z.string(),
  })).optional(),
  inputWidgets: z.array(z.object({
    type: z.string().min(1),
    label: z.string(),
    supportedFieldTypes: z.array(z.string()).min(1),
    component: z.string().min(1),
  })).optional(),
  displayComponents: z.array(z.object({
    type: z.string().min(1),
    label: z.string(),
    supportedFieldTypes: z.array(z.string()).min(1),
    component: z.string().min(1),
  })).optional(),
  viewTypes: z.array(z.object({
    type: z.string(),
    label: z.string(),
    icon: z.string().optional(),
    component: z.string(),
  })).optional(),
});

export type Plugin = z.infer<typeof PluginSchema>;
export type PluginSchemaResponse = z.infer<typeof PluginSchemaResponseSchema>;
export type MigrationStatus = z.infer<typeof MigrationStatusSchema>;
export type SettingsResponse = z.infer<typeof SettingsResponseSchema>;
export type HealthResponse = z.infer<typeof HealthResponseSchema>;
export type PluginPage = z.infer<typeof PluginPageSchema>;
export type PluginPagesResponse = z.infer<typeof PluginPagesResponseSchema>;
export type PluginAssetsResponse = z.infer<typeof PluginAssetsResponseSchema>;
export type WriteSuccessResponse = z.infer<typeof WriteSuccessResponseSchema>;
export type PluginManifest = z.infer<typeof PluginManifestSchema>;
