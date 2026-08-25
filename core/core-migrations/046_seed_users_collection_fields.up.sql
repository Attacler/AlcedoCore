-- Restore missing system fields for the 'users' collection in collection_fields.
-- Migration 024 seeded them into the old JSONB column; migration 027 should have
-- moved them but apparently did not for this collection.

INSERT INTO collection_fields (collection_name, name, display_name, field_type, required, unique_constraint, ordinal_position, is_system, hidden)
VALUES
  ('users', 'email', 'Email', 'string', true, true, 1, true, false),
  ('users', 'display_name', 'Display Name', 'string', false, false, 2, true, false),
  ('users', 'is_admin', 'Administrator', 'boolean', false, false, 3, true, false),
  ('users', 'password_hash', 'Password Hash', 'text', true, false, 4, true, true)
ON CONFLICT (collection_name, name) DO NOTHING;
