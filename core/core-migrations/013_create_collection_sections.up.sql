CREATE TABLE IF NOT EXISTS collection_sections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL,
    name TEXT NOT NULL,
    section_type TEXT NOT NULL DEFAULT 'relational',
    relation_field TEXT,
    view_type TEXT DEFAULT 'table',
    default_filter JSONB DEFAULT NULL,
    display_fields TEXT[] DEFAULT NULL,
    item_limit INTEGER NOT NULL DEFAULT 25,
    ordinal_position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_collection_sections_collection ON collection_sections(collection_name);
