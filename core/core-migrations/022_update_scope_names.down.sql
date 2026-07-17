UPDATE role_scopes SET scope = 'settings.read' WHERE scope = 'settings.read.all';
UPDATE role_scopes SET scope = 'settings.write' WHERE scope = 'settings.write.all';
