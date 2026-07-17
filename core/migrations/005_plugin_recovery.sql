CREATE TABLE plugin_recovery (
    slug VARCHAR(255) PRIMARY KEY REFERENCES plugins(slug) ON DELETE CASCADE,
    restart_count INTEGER NOT NULL DEFAULT 0,
    last_restart_at TIMESTAMP WITH TIME ZONE,
    restart_policy VARCHAR(50) NOT NULL DEFAULT 'always',
    next_restart_at TIMESTAMP WITH TIME ZONE,
    max_restart_attempts INTEGER NOT NULL DEFAULT 10,
    last_error TEXT
);

CREATE INDEX idx_plugin_recovery_restart_policy ON plugin_recovery(restart_policy);
CREATE INDEX idx_plugin_recovery_next_restart_at ON plugin_recovery(next_restart_at);