-- Migrate old 2-level settings scopes to new 3-level scopes
UPDATE role_scopes SET scope = 'settings.read.all' WHERE scope = 'settings.read';
UPDATE role_scopes SET scope = 'settings.write.all' WHERE scope = 'settings.write';
