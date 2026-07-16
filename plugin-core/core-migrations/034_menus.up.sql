CREATE TABLE IF NOT EXISTS menus (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT 'menu',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS menu_sections (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    menu_id UUID NOT NULL REFERENCES menus(id) ON DELETE CASCADE,
    label VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT '',
    visible BOOLEAN NOT NULL DEFAULT true,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS menu_items (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    section_id UUID NOT NULL REFERENCES menu_sections(id) ON DELETE CASCADE,
    parent_item_id UUID REFERENCES menu_items(id) ON DELETE CASCADE,
    label VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT '',
    visible BOOLEAN NOT NULL DEFAULT true,
    route VARCHAR(500),
    url VARCHAR(2000),
    external BOOLEAN NOT NULL DEFAULT false,
    link_type VARCHAR(50) DEFAULT 'custom',
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS menu_roles (
    menu_id UUID NOT NULL REFERENCES menus(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (menu_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_menu_sections_menu_id ON menu_sections(menu_id);
CREATE INDEX IF NOT EXISTS idx_menu_items_section_id ON menu_items(section_id);
CREATE INDEX IF NOT EXISTS idx_menu_items_parent ON menu_items(parent_item_id);
CREATE INDEX IF NOT EXISTS idx_menu_roles_role_id ON menu_roles(role_id);
