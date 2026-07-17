ALTER TABLE plugin_versions ADD COLUMN IF NOT EXISTS pages_synced BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE plugin_versions ADD COLUMN IF NOT EXISTS pages_path VARCHAR(500);
CREATE INDEX IF NOT EXISTS idx_plugin_versions_pages_synced ON plugin_versions(slug, pages_synced) WHERE pages_synced = FALSE;