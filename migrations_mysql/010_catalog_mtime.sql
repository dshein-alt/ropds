-- Add mtime tracking for archive change detection
ALTER TABLE catalogs ADD COLUMN cat_mtime VARCHAR(64) NOT NULL DEFAULT '';
