-- Activity log tables for system-wide and collection-level event tracking.
-- system_logs: tracks setting changes and collection structure changes
-- collection_logs: tracks collection item CRUD operations with field-level diffs

CREATE TABLE IF NOT EXISTS system_logs (
    id BIGSERIAL PRIMARY KEY,
    action VARCHAR(255) NOT NULL,
    target VARCHAR(500) NOT NULL,
    description TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_system_logs_created_at ON system_logs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_system_logs_action ON system_logs(action);
CREATE INDEX IF NOT EXISTS idx_system_logs_target ON system_logs(target);

CREATE TABLE IF NOT EXISTS collection_logs (
    id BIGSERIAL PRIMARY KEY,
    action VARCHAR(255) NOT NULL,
    collection_name VARCHAR(255) NOT NULL,
    item_id JSONB NOT NULL,
    diff JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_collection_logs_created_at ON collection_logs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_collection_logs_action ON collection_logs(action);
CREATE INDEX IF NOT EXISTS idx_collection_logs_collection_name ON collection_logs(collection_name);
CREATE INDEX IF NOT EXISTS idx_collection_logs_item_id ON collection_logs USING gin(item_id);
