ALTER TABLE registries ADD COLUMN IF NOT EXISTS pull_url VARCHAR(2048);

COMMENT ON COLUMN registries.pull_url IS 'Optional URL used for container image pulling. If set, image references use this hostname instead of the api url hostname. Set this when the registry is reachable at a different hostname for pulls (e.g., cluster-internal DNS name vs external API name).';
