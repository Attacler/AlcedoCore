CREATE TABLE IF NOT EXISTS file_folders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    parent_id UUID REFERENCES file_folders(id) ON DELETE CASCADE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_file_folders_parent_id ON file_folders(parent_id);
CREATE INDEX IF NOT EXISTS idx_file_folders_created_by ON file_folders(created_by);

ALTER TABLE file_metadata ADD COLUMN IF NOT EXISTS folder_id UUID REFERENCES file_folders(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_file_metadata_folder_id ON file_metadata(folder_id);
