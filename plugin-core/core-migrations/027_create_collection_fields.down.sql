ALTER TABLE collection_definitions ADD COLUMN fields JSONB NOT NULL DEFAULT '[]'::jsonb;

CREATE INDEX IF NOT EXISTS idx_collection_definitions_fields ON collection_definitions USING gin(fields);

UPDATE collection_definitions cd
SET fields = (
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object(
            'name', cf.name,
            'display_name', cf.display_name,
            'type', cf.field_type,
            'required', cf.required,
            'unique', cf.unique_constraint,
            'default_value', cf.default_value,
            'default', cf.default_value,
            'display_type', cf.display_type,
            'ordinal_position', cf.ordinal_position,
            'related_collection', cf.related_collection,
            'relationship_type', cf.relationship_type,
            'display_field', cf.display_field,
            'inline_parent_fields', cf.inline_parent_fields,
            'is_system', cf.is_system,
            'hidden', cf.hidden,
            'full_width', cf.full_width
        ) ORDER BY cf.ordinal_position
    ), '[]'::jsonb)
    FROM collection_fields cf
    WHERE cf.collection_name = cd.name
);

DROP TABLE IF EXISTS collection_fields CASCADE;
