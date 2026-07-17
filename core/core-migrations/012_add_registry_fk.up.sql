ALTER TABLE plugins ADD COLUMN registry_id INTEGER REFERENCES registries(id);
UPDATE plugins SET registry_id = (SELECT id FROM registries ORDER BY id LIMIT 1) WHERE registry_id IS NULL;
