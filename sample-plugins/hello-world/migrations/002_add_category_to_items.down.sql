DELETE FROM items WHERE name IN ('Release v1.4', 'Future Work');

ALTER TABLE items DROP COLUMN IF EXISTS category;
