-- Add tags field for plugin categorization (CRUD-04)
ALTER TABLE plugins ADD COLUMN IF NOT EXISTS tags JSONB NOT NULL DEFAULT '[]';

-- Index for tag-based filtering
CREATE INDEX idx_plugins_tags ON plugins USING GIN (tags);