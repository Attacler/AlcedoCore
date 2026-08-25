-- Seed layout sections for the 'users' system collection.
-- Migration 040 already created a default layout for every collection,
-- so we only need to insert sections referencing that layout.

-- Section 1: "Basic Info" — display_name + email, 2-column layout
INSERT INTO collection_sections (
    collection_name, name, section_type, display_fields,
    ordinal_position, layout_id, default_filter
)
SELECT
    'users',
    'Basic Info',
    'field_group',
    ARRAY['display_name', 'email'],
    1,
    cl.id,
    '{"_columns": 2, "_field_columns": {"display_name": 1, "email": 2}}'::jsonb
FROM collection_layouts cl
WHERE cl.collection_name = 'users' AND cl.is_default = true
ON CONFLICT DO NOTHING;

-- Section 2: "Access" — is_admin toggle
INSERT INTO collection_sections (
    collection_name, name, section_type, display_fields,
    ordinal_position, layout_id, default_filter
)
SELECT
    'users',
    'Access',
    'field_group',
    ARRAY['is_admin'],
    2,
    cl.id,
    '{}'::jsonb
FROM collection_layouts cl
WHERE cl.collection_name = 'users' AND cl.is_default = true
ON CONFLICT DO NOTHING;
