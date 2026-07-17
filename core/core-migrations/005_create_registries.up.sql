CREATE TABLE IF NOT EXISTS registries (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    url VARCHAR(2048) NOT NULL,
    auth_type VARCHAR(50) NOT NULL DEFAULT 'none',
    username VARCHAR(255),
    password VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_auth_type CHECK (auth_type IN ('none', 'basic', 'bearer'))
);

CREATE INDEX IF NOT EXISTS idx_registries_name ON registries(name);
