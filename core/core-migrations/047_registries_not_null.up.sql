-- Plugin images always pull from a configured registry: backfill the FK and
-- require it thereafter. When no registry exists yet, seed a default `local`
-- one (upgraded to the real host by REGISTRY_URL / LOCAL_REGISTRY_URL at
-- startup via Registry::ensure_default).

INSERT INTO registries (name, url, pull_url, auth_type, username, password, created_at, updated_at)
SELECT 'local', 'http://localhost:5000', NULL, 'none', NULL, NULL, NOW(), NOW()
WHERE NOT EXISTS (SELECT 1 FROM registries);

UPDATE plugins
SET registry_id = (SELECT id FROM registries ORDER BY id LIMIT 1)
WHERE registry_id IS NULL OR registry_id = 0;

ALTER TABLE plugins
    ALTER COLUMN registry_id SET NOT NULL;