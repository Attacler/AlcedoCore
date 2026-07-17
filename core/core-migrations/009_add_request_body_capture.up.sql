-- Add request body capture columns to request_logs for replay debugging
-- Columns added via ALTER TABLE ADD COLUMN IF NOT EXISTS for idempotency

ALTER TABLE request_logs
  ADD COLUMN IF NOT EXISTS request_body TEXT,
  ADD COLUMN IF NOT EXISTS request_headers JSONB,
  ADD COLUMN IF NOT EXISTS request_body_size INTEGER;
