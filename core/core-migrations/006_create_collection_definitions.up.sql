CREATE TABLE IF NOT EXISTS collection_definitions (
    name VARCHAR(59) PRIMARY KEY,
    fields JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_collection_definitions_updated_at
    ON collection_definitions(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_collection_definitions_fields
    ON collection_definitions USING gin(fields);
