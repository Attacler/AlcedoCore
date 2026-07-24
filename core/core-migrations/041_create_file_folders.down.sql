DROP INDEX IF EXISTS idx_file_metadata_folder_id;
ALTER TABLE file_metadata DROP COLUMN IF EXISTS folder_id;
DROP TABLE IF EXISTS file_folders;
