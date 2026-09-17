-- Consolidated per-app-version core schema (net of the former core-001..core-047).
-- Executed unqualified with search_path set to the per-app-version schema.
--
-- Global, non-app-bound tables (registries, plugins, plugin versions, plugin
-- recovery, developer API keys, users) live in core-migrations-global/ and are
-- created in the "alcedo" schema. Because this migration runs with search_path
-- set to the app schema only, foreign keys targeting those tables MUST be
-- schema-qualified (e.g. alcedo.alcedo_users, alcedo.alcedo_plugins).

CREATE TABLE alcedocore_request_logs (
    id SERIAL PRIMARY KEY,
    request_id VARCHAR(255) NOT NULL,
    plugin_slug VARCHAR(255) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    method VARCHAR(10) NOT NULL,
    path VARCHAR(2048) NOT NULL,
    status_code INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    client_ip VARCHAR(45),
    user_agent TEXT,
    request_body TEXT,
    request_headers JSONB,
    request_body_size INTEGER,
    source VARCHAR(20) NOT NULL DEFAULT 'production',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_request_logs_plugin_slug ON alcedocore_request_logs(plugin_slug);
CREATE INDEX idx_alcedocore_request_logs_request_id ON alcedocore_request_logs(request_id);
CREATE INDEX idx_alcedocore_request_logs_created_at ON alcedocore_request_logs(created_at DESC);

CREATE TABLE alcedocore_event_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_slug VARCHAR(255) NOT NULL,
    install_id BIGINT NOT NULL REFERENCES alcedo.alcedo_plugins(id) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL,
    callback_url VARCHAR(500) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(install_id, event_type)
);

CREATE TABLE alcedocore_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) UNIQUE NOT NULL,
    description TEXT,
    is_system BOOLEAN DEFAULT false,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE alcedocore_role_scopes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    role_id UUID NOT NULL REFERENCES alcedocore_roles(id) ON DELETE CASCADE,
    scope VARCHAR(255) NOT NULL,
    UNIQUE(role_id, scope)
);
CREATE INDEX idx_alcedocore_role_scopes_role_id ON alcedocore_role_scopes(role_id);

CREATE TABLE alcedocore_user_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES alcedo.alcedo_users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES alcedocore_roles(id) ON DELETE CASCADE,
    UNIQUE(user_id, role_id)
);
CREATE INDEX idx_alcedocore_user_roles_user_id ON alcedocore_user_roles(user_id);
CREATE INDEX idx_alcedocore_user_roles_role_id ON alcedocore_user_roles(role_id);

CREATE TABLE alcedocore_policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE alcedocore_role_policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    role_id UUID NOT NULL REFERENCES alcedocore_roles(id) ON DELETE CASCADE,
    policy_id UUID NOT NULL REFERENCES alcedocore_policies(id) ON DELETE CASCADE,
    UNIQUE(role_id, policy_id)
);
CREATE INDEX idx_alcedocore_role_policies_role_id ON alcedocore_role_policies(role_id);
CREATE INDEX idx_alcedocore_role_policies_policy_id ON alcedocore_role_policies(policy_id);

CREATE TABLE alcedocore_collection_definitions (
    name VARCHAR(59) PRIMARY KEY,
    display_name TEXT,
    is_system BOOLEAN NOT NULL DEFAULT false,
    plugin_slug VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_collection_definitions_updated_at ON alcedocore_collection_definitions(updated_at DESC);

CREATE TABLE alcedocore_collection_fields (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name VARCHAR(255) NOT NULL REFERENCES alcedocore_collection_definitions(name) ON DELETE CASCADE,
    name VARCHAR(59) NOT NULL,
    display_name VARCHAR(255),
    field_type VARCHAR(50) NOT NULL,
    required BOOLEAN NOT NULL DEFAULT false,
    unique_constraint BOOLEAN NOT NULL DEFAULT false,
    default_value JSONB,
    display_type VARCHAR(50),
    ordinal_position INT NOT NULL DEFAULT 0,
    related_collection VARCHAR(255),
    relationship_type VARCHAR(50),
    display_field VARCHAR(255),
    inline_parent_fields JSONB DEFAULT '[]'::jsonb,
    options JSONB DEFAULT '[]'::jsonb,
    is_system BOOLEAN NOT NULL DEFAULT false,
    hidden BOOLEAN NOT NULL DEFAULT false,
    input_component VARCHAR(50),
    display_component VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);
CREATE INDEX idx_alcedocore_collection_fields_collection ON alcedocore_collection_fields(collection_name);
CREATE INDEX idx_alcedocore_collection_fields_ordinal ON alcedocore_collection_fields(collection_name, ordinal_position);

CREATE TABLE alcedocore_collection_layouts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL REFERENCES alcedocore_collection_definitions(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT false,
    ordinal_position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);
CREATE INDEX idx_alcedocore_collection_layouts_collection ON alcedocore_collection_layouts(collection_name);

CREATE TABLE alcedocore_collection_layout_roles (
    layout_id UUID NOT NULL REFERENCES alcedocore_collection_layouts(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES alcedocore_roles(id) ON DELETE CASCADE,
    UNIQUE(layout_id, role_id)
);
CREATE INDEX idx_alcedocore_collection_layout_roles_layout ON alcedocore_collection_layout_roles(layout_id);
CREATE INDEX idx_alcedocore_collection_layout_roles_role ON alcedocore_collection_layout_roles(role_id);

CREATE TABLE alcedocore_collection_sections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL REFERENCES alcedocore_collection_definitions(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    section_type TEXT NOT NULL DEFAULT 'relational',
    relation_field TEXT,
    view_type TEXT DEFAULT 'table',
    default_filter JSONB DEFAULT NULL,
    display_fields TEXT[] DEFAULT NULL,
    item_limit INTEGER NOT NULL DEFAULT 25,
    ordinal_position INTEGER NOT NULL DEFAULT 0,
    layout_id UUID NOT NULL REFERENCES alcedocore_collection_layouts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_collection_sections_collection ON alcedocore_collection_sections(collection_name);
CREATE INDEX idx_alcedocore_collection_sections_layout ON alcedocore_collection_sections(layout_id);

CREATE TABLE alcedocore_saved_views (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_name TEXT NOT NULL,
    name TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(collection_name, name)
);
CREATE INDEX idx_alcedocore_saved_views_collection ON alcedocore_saved_views(collection_name);
CREATE INDEX idx_alcedocore_saved_views_default ON alcedocore_saved_views(collection_name, is_default) WHERE is_default = true;

CREATE TABLE alcedocore_policy_permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES alcedocore_policies(id) ON DELETE CASCADE,
    collection_name VARCHAR(59) NOT NULL REFERENCES alcedocore_collection_definitions(name) ON DELETE CASCADE,
    action TEXT NOT NULL,
    fields JSONB,
    filter JSONB NOT NULL DEFAULT '[]'::jsonb,
    field_validation JSONB DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(policy_id, collection_name, action)
);

CREATE TABLE alcedocore_plugin_policies (
    plugin_slug VARCHAR(255) NOT NULL,
    policy_id UUID NOT NULL REFERENCES alcedocore_policies(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (plugin_slug, policy_id)
);

CREATE TABLE alcedocore_menus (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT 'menu',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TABLE alcedocore_menu_sections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    menu_id UUID NOT NULL REFERENCES alcedocore_menus(id) ON DELETE CASCADE,
    label VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT '',
    visible BOOLEAN NOT NULL DEFAULT true,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE alcedocore_menu_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    section_id UUID NOT NULL REFERENCES alcedocore_menu_sections(id) ON DELETE CASCADE,
    parent_item_id UUID REFERENCES alcedocore_menu_items(id) ON DELETE CASCADE,
    label VARCHAR(255) NOT NULL,
    icon VARCHAR(100) DEFAULT '',
    visible BOOLEAN NOT NULL DEFAULT true,
    route VARCHAR(500),
    url VARCHAR(2000),
    external BOOLEAN NOT NULL DEFAULT false,
    link_type VARCHAR(50) DEFAULT 'custom',
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE alcedocore_menu_roles (
    menu_id UUID NOT NULL REFERENCES alcedocore_menus(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES alcedocore_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (menu_id, role_id)
);
CREATE INDEX idx_alcedocore_menu_sections_menu_id ON alcedocore_menu_sections(menu_id);
CREATE INDEX idx_alcedocore_menu_items_section_id ON alcedocore_menu_items(section_id);
CREATE INDEX idx_alcedocore_menu_items_parent ON alcedocore_menu_items(parent_item_id);
CREATE INDEX idx_alcedocore_menu_roles_role_id ON alcedocore_menu_roles(role_id);

CREATE TABLE alcedocore_system_settings (
    key VARCHAR(255) PRIMARY KEY,
    value JSONB NOT NULL,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE alcedocore_system_logs (
    id BIGSERIAL PRIMARY KEY,
    action VARCHAR(255) NOT NULL,
    target VARCHAR(500) NOT NULL,
    description TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    request_id VARCHAR(36),
    actor_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_system_logs_created_at ON alcedocore_system_logs(created_at DESC);
CREATE INDEX idx_alcedocore_system_logs_action ON alcedocore_system_logs(action);
CREATE INDEX idx_alcedocore_system_logs_target ON alcedocore_system_logs(target);
CREATE INDEX idx_alcedocore_system_logs_request_id ON alcedocore_system_logs(request_id);
CREATE INDEX idx_alcedocore_system_logs_actor_id ON alcedocore_system_logs(actor_id);

CREATE TABLE alcedocore_collection_logs (
    id BIGSERIAL PRIMARY KEY,
    action VARCHAR(255) NOT NULL,
    collection_name VARCHAR(255) NOT NULL,
    item_id JSONB NOT NULL,
    diff JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    request_id VARCHAR(36),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_collection_logs_created_at ON alcedocore_collection_logs(created_at DESC);
CREATE INDEX idx_alcedocore_collection_logs_action ON alcedocore_collection_logs(action);
CREATE INDEX idx_alcedocore_collection_logs_collection_name ON alcedocore_collection_logs(collection_name);
CREATE INDEX idx_alcedocore_collection_logs_item_id ON alcedocore_collection_logs USING gin(item_id);
CREATE INDEX idx_alcedocore_collection_logs_request_id ON alcedocore_collection_logs(request_id);

CREATE TABLE alcedocore_host_calls (
    id BIGSERIAL PRIMARY KEY,
    parent_request_id VARCHAR(255) NOT NULL,
    action_type VARCHAR(50) NOT NULL,
    args_summary TEXT NOT NULL DEFAULT '',
    result_summary TEXT NOT NULL DEFAULT '',
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_host_calls_parent_request_id ON alcedocore_host_calls(parent_request_id);
CREATE INDEX idx_alcedocore_host_calls_created_at ON alcedocore_host_calls(created_at DESC);
CREATE INDEX idx_alcedocore_host_calls_action_type ON alcedocore_host_calls(action_type);

CREATE TABLE alcedocore_file_folders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    parent_id UUID REFERENCES alcedocore_file_folders(id) ON DELETE CASCADE,
    created_by UUID REFERENCES alcedo.alcedo_users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_file_folders_parent_id ON alcedocore_file_folders(parent_id);
CREATE INDEX idx_alcedocore_file_folders_created_by ON alcedocore_file_folders(created_by);

CREATE TABLE alcedocore_file_metadata (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    storage_provider TEXT NOT NULL,
    storage_path TEXT NOT NULL UNIQUE,
    sha256 TEXT,
    alt_text TEXT,
    folder_id UUID REFERENCES alcedocore_file_folders(id) ON DELETE SET NULL,
    uploaded_by UUID REFERENCES alcedo.alcedo_users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alcedocore_file_metadata_uploaded_by ON alcedocore_file_metadata(uploaded_by);
CREATE INDEX idx_alcedocore_file_metadata_created_at ON alcedocore_file_metadata(created_at DESC);
CREATE INDEX idx_alcedocore_file_metadata_folder_id ON alcedocore_file_metadata(folder_id);

CREATE TABLE alcedocore_item_files (
    item_id UUID NOT NULL,
    collection_name VARCHAR(59) NOT NULL,
    field_name VARCHAR(255) NOT NULL,
    file_id UUID NOT NULL REFERENCES alcedocore_file_metadata(id) ON DELETE CASCADE,
    ordinal_position INT NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, field_name, file_id)
);
CREATE INDEX idx_alcedocore_item_files_file_id ON alcedocore_item_files(file_id);
CREATE INDEX idx_alcedocore_item_files_item ON alcedocore_item_files(item_id, collection_name);

INSERT INTO alcedocore_system_settings (key, value, description) VALUES
('menu_sections',
 '[{"id":"collections","label":"Collections","icon":"folder","visible":true,"items":[]},{"id":"plugins","label":"Plugins","icon":"extension","visible":true,"items":[]},{"id":"settings","label":"System Settings","icon":"settings","visible":true,"items":[]}]'::jsonb,
 'Default sidebar menu sections visible after fresh install')
ON CONFLICT (key) DO NOTHING;

INSERT INTO alcedocore_collection_definitions (name, display_name, is_system, plugin_slug)
VALUES ('alcedo_users', 'Users', true, '_system')
ON CONFLICT (name) DO NOTHING;

INSERT INTO alcedocore_collection_fields
    (collection_name, name, display_name, field_type, required, unique_constraint, ordinal_position, is_system, hidden)
VALUES
  ('alcedo_users', 'email', 'Email', 'string', true, true, 1, true, false),
  ('alcedo_users', 'display_name', 'Display Name', 'string', false, false, 2, true, false),
  ('alcedo_users', 'is_admin', 'Administrator', 'boolean', false, false, 3, true, false),
  ('alcedo_users', 'password_hash', 'Password Hash', 'text', true, false, 4, true, true)
ON CONFLICT (collection_name, name) DO NOTHING;

INSERT INTO alcedocore_collection_layouts (collection_name, name, is_default, ordinal_position)
VALUES ('alcedo_users', 'Default', true, 0)
ON CONFLICT (collection_name, name) DO NOTHING;

INSERT INTO alcedocore_collection_sections
    (collection_name, name, section_type, display_fields, ordinal_position, layout_id, default_filter)
SELECT 'alcedo_users', 'Basic Info', 'field_group',
       ARRAY['display_name', 'email'], 1, cl.id,
       '{"_columns": 2, "_field_columns": {"display_name": 1, "email": 2}}'::jsonb
FROM alcedocore_collection_layouts cl
WHERE cl.collection_name = 'alcedo_users' AND cl.is_default = true;
INSERT INTO alcedocore_collection_sections
    (collection_name, name, section_type, display_fields, ordinal_position, layout_id, default_filter)
SELECT 'alcedo_users', 'Access', 'field_group',
       ARRAY['is_admin'], 2, cl.id, '{}'::jsonb
FROM alcedocore_collection_layouts cl
WHERE cl.collection_name = 'alcedo_users' AND cl.is_default = true;
