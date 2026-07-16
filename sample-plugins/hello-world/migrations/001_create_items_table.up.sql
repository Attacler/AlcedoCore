CREATE TABLE IF NOT EXISTS items (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO items (name, description) VALUES
    ('Welcome Item', 'This item was created by the hello-world migration'),
    ('Sample Data', 'A sample row to demonstrate the query proxy endpoint'),
    ('Migration Demo', 'Proof that plugin migrations work end-to-end');
