ALTER TABLE users ADD COLUMN allow_upload INTEGER NOT NULL DEFAULT 0;
UPDATE users SET allow_upload = 1 WHERE is_superuser = 1;
