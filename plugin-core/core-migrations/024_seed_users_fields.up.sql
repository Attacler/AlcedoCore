UPDATE collection_definitions SET fields = '[
  {"name": "email", "type": "string", "required": true, "unique": true, "is_system": true},
  {"name": "display_name", "type": "string", "is_system": true},
  {"name": "is_admin", "type": "boolean", "is_system": true},
  {"name": "password_hash", "type": "text", "required": true, "is_system": true, "hidden": true}
]'::jsonb WHERE name = 'users';
