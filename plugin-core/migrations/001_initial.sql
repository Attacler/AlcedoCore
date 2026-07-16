-- Plugin definitions
CREATE TABLE plugins (
    slug VARCHAR(255) PRIMARY KEY,
    image VARCHAR(500) NOT NULL,
    env JSONB NOT NULL DEFAULT '{}',
    resources JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Plugin versions (can have multiple versions, one active)
CREATE TABLE plugin_versions (
    slug VARCHAR(255),
    version VARCHAR(100),
    container_id VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'stopped',
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    deployed_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    public_synced BOOLEAN NOT NULL DEFAULT FALSE,
    public_path VARCHAR(500),
    PRIMARY KEY (slug, version),
    FOREIGN KEY (slug) REFERENCES plugins(slug) ON DELETE CASCADE
);

-- Index for fast active version lookup
CREATE INDEX idx_plugin_versions_active ON plugin_versions(slug, is_active) WHERE is_active = TRUE;