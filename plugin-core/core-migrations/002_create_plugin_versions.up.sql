CREATE TABLE IF NOT EXISTS plugin_versions (
    slug VARCHAR(255),
    version VARCHAR(100),
    container_id VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'stopped',
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    deployed_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    public_synced BOOLEAN NOT NULL DEFAULT FALSE,
    public_path VARCHAR(500),
    pages_synced BOOLEAN NOT NULL DEFAULT FALSE,
    pages_path VARCHAR(500),
    PRIMARY KEY (slug, version),
    FOREIGN KEY (slug) REFERENCES plugins(slug) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plugin_versions_active
    ON plugin_versions(slug, is_active) WHERE is_active = TRUE;

CREATE INDEX IF NOT EXISTS idx_plugin_versions_draining
    ON plugin_versions(status) WHERE status = 'draining';
