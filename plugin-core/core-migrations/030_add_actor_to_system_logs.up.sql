ALTER TABLE system_logs ADD COLUMN IF NOT EXISTS actor_id UUID;
CREATE INDEX IF NOT EXISTS idx_system_logs_actor_id ON system_logs(actor_id);
