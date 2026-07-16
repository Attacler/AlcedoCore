ALTER TABLE plugin_versions ADD COLUMN IF NOT EXISTS public_synced BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE plugin_versions ADD COLUMN IF NOT EXISTS public_path VARCHAR(500);
CREATE INDEX IF NOT EXISTS idx_plugin_versions_public_synced ON plugin_versions(slug, public_synced) WHERE public_synced = FALSE;