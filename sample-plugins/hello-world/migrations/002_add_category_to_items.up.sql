ALTER TABLE items ADD COLUMN IF NOT EXISTS category VARCHAR(100) NOT NULL DEFAULT 'general';

UPDATE items SET category = 'onboarding' WHERE name = 'Welcome Item';
UPDATE items SET category = 'demo' WHERE name = 'Sample Data';
UPDATE items SET category = 'demo' WHERE name = 'Migration Demo';

INSERT INTO items (name, description, category) VALUES
    ('Release v1.4', 'This item was added by the second migration', 'milestone'),
    ('Future Work', 'Planned improvements for the next milestone', 'roadmap');
