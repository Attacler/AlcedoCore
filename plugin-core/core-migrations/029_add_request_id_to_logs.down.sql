DROP INDEX IF EXISTS idx_system_logs_request_id;
DROP INDEX IF EXISTS idx_collection_logs_request_id;
ALTER TABLE system_logs DROP COLUMN IF EXISTS request_id;
ALTER TABLE collection_logs DROP COLUMN IF EXISTS request_id;
