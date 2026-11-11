use uuid::Uuid;

/// Fired after a file has been uploaded and its metadata row committed.
#[derive(Clone)]
pub struct FileUploaded {
    pub file_id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
}

/// Fired after a file (and its metadata row) has been deleted.
#[derive(Clone)]
pub struct FileDeleted {
    pub file_id: Uuid,
    pub filename: String,
}
