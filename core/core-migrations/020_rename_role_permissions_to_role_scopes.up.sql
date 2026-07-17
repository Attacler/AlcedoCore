ALTER TABLE role_permissions RENAME TO role_scopes;
ALTER TABLE role_scopes RENAME COLUMN permission TO scope;
