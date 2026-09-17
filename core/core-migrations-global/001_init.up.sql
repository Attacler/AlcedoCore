-- Global core schema: tables that are NOT app-bound. Executed unqualified
-- with search_path set to "alcedo", public.

CREATE TABLE alcedo_registries (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    url VARCHAR(2048) NOT NULL,
    auth_type VARCHAR(50) NOT NULL DEFAULT 'none',
    username VARCHAR(255),
    password VARCHAR(255),
    pull_url VARCHAR(2048),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_auth_type CHECK (auth_type IN ('none', 'basic', 'bearer'))
);
CREATE INDEX idx_alcedo_registries_name ON alcedo_registries(name);
INSERT INTO alcedo_registries (id, auth_type, name, url)
VALUES (0, 'none', 'AlcedoSystemPlugins', '');
COMMENT ON COLUMN alcedo_registries.pull_url IS 'Optional URL used for container image pulling. If set, image references use this hostname instead of the api url hostname. Set this when the registry is reachable at a different hostname for pulls (e.g., cluster-internal DNS name vs external API name).';

CREATE TABLE alcedo_plugins (
    id BIGSERIAL PRIMARY KEY,
    slug VARCHAR(255) NOT NULL,
    app_version_id INTEGER REFERENCES alcedo_apps_versions(id) ON DELETE CASCADE,
    version_id INTEGER REFERENCES alcedo_versions(id) ON DELETE CASCADE,
    image VARCHAR(500) NOT NULL,
    plugin_type VARCHAR(50) NOT NULL DEFAULT 'docker',
    system_plugin BOOLEAN NOT NULL DEFAULT FALSE,
    env JSONB NOT NULL DEFAULT '{}',
    resources JSONB NOT NULL DEFAULT '{}',
    display_name VARCHAR(500),
    description TEXT,
    pages JSONB NOT NULL DEFAULT '[]',
    endpoints JSONB NOT NULL DEFAULT '[]',
    documentation JSONB NOT NULL DEFAULT '[]',
    settings_schema JSONB NOT NULL DEFAULT '{}',
    settings JSONB NOT NULL DEFAULT '{}',
    tags JSONB NOT NULL DEFAULT '[]',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    registry_id INTEGER NOT NULL REFERENCES alcedo_registries(id),
    requested_scopes JSONB NOT NULL DEFAULT '[]',
    granted_scopes JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT chk_alcedo_plugins_scope
        CHECK (app_version_id IS NULL OR version_id IS NULL)
);
CREATE UNIQUE INDEX uq_alcedo_plugins_global
    ON alcedo_plugins(slug)
    WHERE app_version_id IS NULL AND version_id IS NULL;
CREATE UNIQUE INDEX uq_alcedo_plugins_version
    ON alcedo_plugins(slug, version_id)
    WHERE version_id IS NOT NULL;
CREATE UNIQUE INDEX uq_alcedo_plugins_app
    ON alcedo_plugins(slug, app_version_id)
    WHERE app_version_id IS NOT NULL;
CREATE INDEX idx_alcedo_plugins_version_id ON alcedo_plugins(version_id) WHERE version_id IS NOT NULL;
CREATE INDEX idx_alcedo_plugins_app_version_id ON alcedo_plugins(app_version_id) WHERE app_version_id IS NOT NULL;
CREATE INDEX idx_alcedo_plugins_slug ON alcedo_plugins(slug);

CREATE TABLE alcedo_plugin_versions (
    id BIGSERIAL PRIMARY KEY,
    install_id BIGINT NOT NULL REFERENCES alcedo_plugins(id) ON DELETE CASCADE,
    slug VARCHAR(255) NOT NULL,
    version VARCHAR(100) NOT NULL,
    deployment_id VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'stopped',
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    deployed_at TIMESTAMPTZ DEFAULT NOW(),
    public_synced BOOLEAN NOT NULL DEFAULT FALSE,
    public_path VARCHAR(500),
    pages_synced BOOLEAN NOT NULL DEFAULT FALSE,
    pages_path VARCHAR(500),
    CONSTRAINT uq_alcedo_plugin_versions_install_version UNIQUE (install_id, version)
);
CREATE INDEX idx_alcedo_plugin_versions_active ON alcedo_plugin_versions(slug, install_id, is_active) WHERE is_active = TRUE;
CREATE INDEX idx_alcedo_plugin_versions_draining ON alcedo_plugin_versions(status) WHERE status = 'draining';

CREATE TABLE alcedo_plugin_recovery (
    install_id BIGINT PRIMARY KEY REFERENCES alcedo_plugins(id) ON DELETE CASCADE,
    restart_count INTEGER NOT NULL DEFAULT 0,
    last_restart_at TIMESTAMPTZ,
    restart_policy VARCHAR(50) NOT NULL DEFAULT 'always',
    next_restart_at TIMESTAMPTZ,
    max_restart_attempts INTEGER NOT NULL DEFAULT 10,
    last_error TEXT
);
CREATE INDEX idx_alcedo_plugin_recovery_policy ON alcedo_plugin_recovery(restart_policy);
CREATE INDEX idx_alcedo_plugin_recovery_next ON alcedo_plugin_recovery(next_restart_at);

CREATE TABLE alcedo_developer_api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    version_id INTEGER NOT NULL REFERENCES alcedo_versions(id),
    key_hash TEXT NOT NULL,
    key_prefix VARCHAR(10) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX idx_alcedo_dev_keys_prefix ON alcedo_developer_api_keys(key_prefix);
CREATE INDEX idx_alcedo_dev_keys_version ON alcedo_developer_api_keys(version_id);

CREATE TABLE alcedo_users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name VARCHAR(255),
    is_admin BOOLEAN DEFAULT false,
    last_login_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
CREATE INDEX idx_alcedo_users_email ON alcedo_users(email);
