CREATE TABLE IF NOT EXISTS saved_views (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);

CREATE INDEX IF NOT EXISTS idx_saved_views_collection
    ON saved_views(collection_name);

CREATE INDEX IF NOT EXISTS idx_saved_views_default
    ON saved_views(collection_name, is_default) WHERE is_default = true;
