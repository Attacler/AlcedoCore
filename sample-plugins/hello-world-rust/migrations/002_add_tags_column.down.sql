DELETE FROM demo_items WHERE name IN ('KV Integration', 'Settings Demo');

ALTER TABLE demo_items DROP COLUMN IF EXISTS tags;
