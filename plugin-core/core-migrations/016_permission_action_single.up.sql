-- Migration: Split multi-action permission rows into single-action rows
-- Adds action-specific columns: action (single), filter (renamed), field_validation (new)

CREATE TABLE IF NOT EXISTS policy_permissions_new (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    collection_name VARCHAR(59) NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    action TEXT NOT NULL,
    fields JSONB,
    filter JSONB NOT NULL DEFAULT '[]'::jsonb,
    field_validation JSONB DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(policy_id, collection_name, action)
);

INSERT INTO policy_permissions_new
    (id, policy_id, collection_name, action, fields, filter, created_at, updated_at)
SELECT gen_random_uuid(), policy_id, collection_name, unnest(actions), fields, filters, created_at, updated_at
FROM policy_permissions;

DROP TABLE policy_permissions;

ALTER TABLE policy_permissions_new RENAME TO policy_permissions;
