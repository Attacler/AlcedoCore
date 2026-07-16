CREATE TABLE IF NOT EXISTS demo_items (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    category VARCHAR(100) NOT NULL DEFAULT 'general',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO demo_items (name, description, category) VALUES
    ('Rust Welcome', 'Created by the hello-world-rust migration', 'onboarding'),
    ('Demo Record', 'A sample row to demonstrate DB query proxy', 'demo'),
    ('Migration Test', 'End-to-end migration via Rust SDK', 'demo');
