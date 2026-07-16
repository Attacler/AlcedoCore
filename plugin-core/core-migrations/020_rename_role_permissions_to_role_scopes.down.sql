ALTER TABLE role_scopes RENAME COLUMN scope TO permission;
ALTER TABLE role_scopes RENAME TO role_permissions;
