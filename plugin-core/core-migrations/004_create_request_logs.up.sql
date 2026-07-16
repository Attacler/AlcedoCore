CREATE TABLE IF NOT EXISTS request_logs (
    id SERIAL PRIMARY KEY,
    request_id VARCHAR(255) NOT NULL,
    plugin_slug VARCHAR(255) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    method VARCHAR(10) NOT NULL,
    path VARCHAR(2048) NOT NULL,
    status_code INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    client_ip VARCHAR(45),
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (plugin_slug) REFERENCES plugins(slug) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_request_logs_plugin_slug
    ON request_logs(plugin_slug);

CREATE INDEX IF NOT EXISTS idx_request_logs_request_id
    ON request_logs(request_id);

CREATE INDEX IF NOT EXISTS idx_request_logs_created_at
    ON request_logs(created_at DESC);
