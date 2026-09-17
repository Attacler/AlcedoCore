-- Rollback for the per-app-version core schema. Only drops app-bound tables.
-- Global, non-app-bound tables (registries, plugins, plugin versions, plugin
-- recovery, developer API keys, users) live in the "alcedo" schema and are
-- dropped by core-migrations-global/, not here.
DROP TABLE IF EXISTS alcedocore_item_files CASCADE;
DROP TABLE IF EXISTS alcedocore_file_metadata CASCADE;
DROP TABLE IF EXISTS alcedocore_file_folders CASCADE;
DROP TABLE IF EXISTS alcedocore_host_calls CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_logs CASCADE;
DROP TABLE IF EXISTS alcedocore_system_logs CASCADE;
DROP TABLE IF EXISTS alcedocore_system_settings CASCADE;
DROP TABLE IF EXISTS alcedocore_menu_roles CASCADE;
DROP TABLE IF EXISTS alcedocore_menu_items CASCADE;
DROP TABLE IF EXISTS alcedocore_menu_sections CASCADE;
DROP TABLE IF EXISTS alcedocore_menus CASCADE;
DROP TABLE IF EXISTS alcedocore_plugin_policies CASCADE;
DROP TABLE IF EXISTS alcedocore_policy_permissions CASCADE;
DROP TABLE IF EXISTS alcedocore_policies CASCADE;
DROP TABLE IF EXISTS alcedocore_saved_views CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_sections CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_layout_roles CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_layouts CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_fields CASCADE;
DROP TABLE IF EXISTS alcedocore_collection_definitions CASCADE;
DROP TABLE IF EXISTS alcedocore_role_policies CASCADE;
DROP TABLE IF EXISTS alcedocore_user_roles CASCADE;
DROP TABLE IF EXISTS alcedocore_role_scopes CASCADE;
DROP TABLE IF EXISTS alcedocore_roles CASCADE;
DROP TABLE IF EXISTS alcedocore_event_subscriptions CASCADE;
DROP TABLE IF EXISTS alcedocore_request_logs CASCADE;
