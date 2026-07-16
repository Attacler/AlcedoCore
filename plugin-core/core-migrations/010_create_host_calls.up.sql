CREATE TABLE IF NOT EXISTS host_calls (
    id BIGSERIAL PRIMARY KEY,
    parent_request_id VARCHAR(255) NOT NULL,
    action_type VARCHAR(50) NOT NULL,
    args_summary TEXT NOT NULL DEFAULT '',
    result_summary TEXT NOT NULL DEFAULT '',
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_host_calls_parent_request_id
    ON host_calls(parent_request_id);

CREATE INDEX IF NOT EXISTS idx_host_calls_created_at
    ON host_calls(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_host_calls_action_type
    ON host_calls(action_type);
