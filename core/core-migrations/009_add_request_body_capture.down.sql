-- Rollback: remove request body capture columns
ALTER TABLE request_logs
  DROP COLUMN IF EXISTS request_body,
  DROP COLUMN IF EXISTS request_headers,
  DROP COLUMN IF EXISTS request_body_size;
