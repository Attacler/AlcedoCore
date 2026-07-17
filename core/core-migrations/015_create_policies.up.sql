-- Policy management tables for plugin collection permissions
-- Policies group permission rules that can be reused across plugins

CREATE TABLE IF NOT EXISTS policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS policy_permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    collection_name VARCHAR(59) NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    actions TEXT[] NOT NULL DEFAULT '{}',
    fields JSONB,
    filters JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(policy_id, collection_name)
);

CREATE TABLE IF NOT EXISTS plugin_policies (
    plugin_slug VARCHAR(255) NOT NULL REFERENCES plugins(slug) ON DELETE CASCADE,
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (plugin_slug, policy_id)
);
