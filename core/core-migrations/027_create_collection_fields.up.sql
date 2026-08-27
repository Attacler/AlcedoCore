CREATE TABLE collection_fields (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name VARCHAR(255) NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    name VARCHAR(59) NOT NULL,
    display_name VARCHAR(255),
    field_type VARCHAR(50) NOT NULL,
    required BOOLEAN NOT NULL DEFAULT false,
    unique_constraint BOOLEAN NOT NULL DEFAULT false,
    default_value JSONB,
    display_type VARCHAR(50),
    ordinal_position INT NOT NULL DEFAULT 0,
    related_collection VARCHAR(255),
    relationship_type VARCHAR(50),
    display_field VARCHAR(255),
    inline_parent_fields JSONB DEFAULT '[]'::jsonb,
    options JSONB DEFAULT '[]'::jsonb,
    is_system BOOLEAN NOT NULL DEFAULT false,
    hidden BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);

CREATE INDEX idx_collection_fields_collection_name ON collection_fields(collection_name);
CREATE INDEX idx_collection_fields_ordinal ON collection_fields(collection_name, ordinal_position);

INSERT INTO collection_fields (
    collection_name, name, display_name, field_type, required, unique_constraint,
    default_value, display_type, ordinal_position, related_collection,
    relationship_type, display_field, inline_parent_fields, is_system, hidden
)
SELECT
    cd.name,
    f.value->>'name',
    f.value->>'display_name',
    COALESCE(f.value->>'type', 'string'),
    COALESCE((f.value->>'required')::boolean, false),
    COALESCE((f.value->>'unique')::boolean, false),
    COALESCE(f.value->'default', f.value->'default_value')::jsonb,
    f.value->>'display_type',
    (row_number() OVER (PARTITION BY cd.name ORDER BY (f.value->>'ordinal_position')::int, f.ordinality))::int,
    f.value->>'related_collection',
    f.value->>'relationship_type',
    f.value->>'display_field',
    COALESCE(f.value->'inline_parent_fields', '[]'::jsonb),
    COALESCE((f.value->>'is_system')::boolean, false),
    COALESCE((f.value->>'hidden')::boolean, false)
FROM collection_definitions cd,
LATERAL jsonb_array_elements(cd.fields) WITH ORDINALITY AS f(value, ordinality);

ALTER TABLE collection_definitions DROP COLUMN fields;
DROP INDEX IF EXISTS idx_collection_definitions_fields;
