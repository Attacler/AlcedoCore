CREATE TABLE IF NOT EXISTS role_policies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    UNIQUE(role_id, policy_id)
);
CREATE INDEX IF NOT EXISTS idx_role_policies_role_id ON role_policies(role_id);
CREATE INDEX IF NOT EXISTS idx_role_policies_policy_id ON role_policies(policy_id);
