CREATE TABLE triggers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    function_id UUID REFERENCES functions(id) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL,
    collection_filter VARCHAR(255),
    field_filter VARCHAR(255),
    conditions JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
