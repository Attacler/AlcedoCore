-- Create system_settings table for app-wide configuration storage
CREATE TABLE IF NOT EXISTS system_settings (
    key VARCHAR(255) PRIMARY KEY,
    value JSONB NOT NULL,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed default menu sections so they appear after fresh install or migration
INSERT INTO system_settings (key, value, description) VALUES
('menu_sections', '[{"id":"collections","label":"Collections","icon":"folder","visible":true,"items":[]},{"id":"plugins","label":"Plugins","icon":"extension","visible":true,"items":[]},{"id":"settings","label":"System Settings","icon":"settings","visible":true,"items":[]}]'::jsonb, 'Default sidebar menu sections visible after fresh install')
ON CONFLICT (key) DO NOTHING;
