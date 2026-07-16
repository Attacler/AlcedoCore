CREATE TABLE IF NOT EXISTS event_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_slug VARCHAR(255) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    callback_url VARCHAR(500) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_slug, event_type)
);
