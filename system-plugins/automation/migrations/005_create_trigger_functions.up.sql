CREATE TABLE IF NOT EXISTS trigger_functions (
    trigger_id UUID NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    function_id UUID NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (trigger_id, function_id)
);

-- Migrate existing data: each trigger's function_id becomes one row
INSERT INTO trigger_functions (trigger_id, function_id)
SELECT id, function_id FROM triggers WHERE function_id IS NOT NULL;
