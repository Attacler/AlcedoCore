CREATE TABLE item_files (
    item_id UUID NOT NULL,
    collection_name VARCHAR(59) NOT NULL,
    field_name VARCHAR(255) NOT NULL,
    file_id UUID NOT NULL REFERENCES file_metadata(id) ON DELETE CASCADE,
    ordinal_position INT NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, field_name, file_id)
);

CREATE INDEX idx_item_files_file_id ON item_files(file_id);
CREATE INDEX idx_item_files_item ON item_files(item_id, collection_name);
