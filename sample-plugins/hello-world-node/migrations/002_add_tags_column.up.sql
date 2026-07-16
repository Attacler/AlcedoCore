ALTER TABLE demo_items ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';

UPDATE demo_items SET tags = ARRAY['welcome', 'first'] WHERE name = 'Node.js Welcome';
UPDATE demo_items SET tags = ARRAY['sample', 'demo'] WHERE name = 'Demo Record';
UPDATE demo_items SET tags = ARRAY['test', 'migration'] WHERE name = 'Migration Test';

INSERT INTO demo_items (name, description, category, tags) VALUES
    ('KV Integration', 'Demonstrates KV store from Node.js plugin', 'integration', ARRAY['kv', 'demo']),
    ('Settings Demo', 'Shows settings access pattern via SDK', 'integration', ARRAY['settings', 'demo']);
