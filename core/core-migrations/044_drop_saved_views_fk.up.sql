-- Allow saved views for any collection_name (management entities like roles,
-- policies, registries, plugins are not rows in collection_definitions).
DO $$ DECLARE cname text;
BEGIN
    SELECT conname INTO cname FROM pg_constraint
        WHERE conrelid = 'saved_views'::regclass
          AND contype = 'f'
          AND confrelid = 'collection_definitions'::regclass
        LIMIT 1;
    IF cname IS NOT NULL THEN
        EXECUTE format('ALTER TABLE saved_views DROP CONSTRAINT %I', cname);
    END IF;
END $$;
