DELETE FROM collection_fields
WHERE collection_name = 'users'
  AND name IN ('email', 'display_name', 'is_admin', 'password_hash');
