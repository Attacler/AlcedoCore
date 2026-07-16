-- Clean up existing orphans first (safe to run multiple times)
DELETE FROM collection_sections cs
WHERE NOT EXISTS (
    SELECT 1 FROM collection_definitions cd WHERE cd.name = cs.collection_name
);

ALTER TABLE collection_sections ADD CONSTRAINT fk_collection_sections_collection
    FOREIGN KEY (collection_name) REFERENCES collection_definitions(name) ON DELETE CASCADE;
