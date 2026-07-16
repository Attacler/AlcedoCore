ALTER TABLE plugins ADD COLUMN requested_scopes JSONB NOT NULL DEFAULT '[]';
ALTER TABLE plugins ADD COLUMN granted_scopes JSONB NOT NULL DEFAULT '[]';
