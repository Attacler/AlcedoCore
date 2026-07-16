ALTER TABLE system_logs ADD COLUMN IF NOT EXISTS request_id VARCHAR(36);
ALTER TABLE collection_logs ADD COLUMN IF NOT EXISTS request_id VARCHAR(36);

CREATE INDEX IF NOT EXISTS idx_system_logs_request_id ON system_logs(request_id);
CREATE INDEX IF NOT EXISTS idx_collection_logs_request_id ON collection_logs(request_id);
