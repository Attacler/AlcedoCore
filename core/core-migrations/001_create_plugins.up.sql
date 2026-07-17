CREATE TABLE IF NOT EXISTS plugins (
    slug VARCHAR(255) PRIMARY KEY,
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
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
