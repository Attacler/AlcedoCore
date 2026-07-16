DELETE FROM collection_definitions WHERE is_system = true;
ALTER TABLE collection_definitions DROP COLUMN IF EXISTS plugin_slug;
ALTER TABLE collection_definitions DROP COLUMN IF EXISTS is_system;
