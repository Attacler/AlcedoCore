CREATE TABLE IF NOT EXISTS collection_layouts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT false,
    ordinal_position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);

CREATE INDEX IF NOT EXISTS idx_collection_layouts_collection ON collection_layouts(collection_name);

CREATE TABLE IF NOT EXISTS collection_layout_roles (
    layout_id UUID NOT NULL REFERENCES collection_layouts(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    UNIQUE(layout_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_collection_layout_roles_layout ON collection_layout_roles(layout_id);
CREATE INDEX IF NOT EXISTS idx_collection_layout_roles_role ON collection_layout_roles(role_id);
