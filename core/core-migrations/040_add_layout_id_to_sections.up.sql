ALTER TABLE collection_sections ADD COLUMN IF NOT EXISTS layout_id UUID REFERENCES collection_layouts(id) ON DELETE CASCADE;

-- Backfill: create a "Default" layout for every collection that has sections
INSERT INTO collection_layouts (collection_name, name, is_default, ordinal_position)
SELECT DISTINCT collection_name, 'Default', true, 0
FROM collection_sections
WHERE collection_name IS NOT NULL;

-- Create default layouts for collections that have no sections yet
INSERT INTO collection_layouts (collection_name, name, is_default, ordinal_position)
SELECT cd.name, 'Default', true, 0
FROM collection_definitions cd
WHERE NOT EXISTS (
    SELECT 1 FROM collection_layouts cl WHERE cl.collection_name = cd.name
);

-- Assign all existing sections to their collection's default layout
UPDATE collection_sections cs
SET layout_id = cl.id
FROM collection_layouts cl
WHERE cl.collection_name = cs.collection_name
  AND cl.is_default = true
  AND cs.layout_id IS NULL;

ALTER TABLE collection_sections ALTER COLUMN layout_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_collection_sections_layout ON collection_sections(layout_id);
