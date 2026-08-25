-- Remove seeded sections for the 'users' collection
DELETE FROM collection_sections
WHERE collection_name = 'users'
  AND name IN ('Basic Info', 'Access');
