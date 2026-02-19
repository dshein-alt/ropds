-- Add mtime tracking for archive change detection
ALTER TABLE catalogs ADD COLUMN cat_mtime TEXT NOT NULL DEFAULT '';
