ALTER TABLE saved_views
    ADD CONSTRAINT saved_views_collection_name_fkey
    FOREIGN KEY (collection_name)
    REFERENCES collection_definitions(name)
    ON DELETE CASCADE;
