ALTER TABLE collection_definitions ADD COLUMN IF NOT EXISTS is_system BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE collection_definitions ADD COLUMN IF NOT EXISTS plugin_slug VARCHAR(255);

INSERT INTO collection_definitions (name, display_name, is_system, plugin_slug)
VALUES ('users', 'Users', true, '_system')
ON CONFLICT (name) DO NOTHING;
