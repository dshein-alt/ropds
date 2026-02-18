-- Add UNIQUE indexes for concurrency-safe INSERT OR IGNORE.
-- First deduplicate any existing rows.

-- Deduplicate authors: relink books to surviving (lowest-id) author
UPDATE book_authors SET author_id = (
    SELECT MIN(a2.id) FROM authors a2
    WHERE a2.full_name = (SELECT full_name FROM authors WHERE id = book_authors.author_id)
) WHERE author_id NOT IN (SELECT MIN(id) FROM authors GROUP BY full_name);
DELETE FROM authors WHERE id NOT IN (SELECT MIN(id) FROM authors GROUP BY full_name);

-- Deduplicate series: relink books to surviving (lowest-id) series
UPDATE book_series SET series_id = (
    SELECT MIN(s2.id) FROM series s2
    WHERE s2.ser_name = (SELECT ser_name FROM series WHERE id = book_series.series_id)
) WHERE series_id NOT IN (SELECT MIN(id) FROM series GROUP BY ser_name);
DELETE FROM series WHERE id NOT IN (SELECT MIN(id) FROM series GROUP BY ser_name);

-- Deduplicate catalogs (should already be unique in practice)
DELETE FROM catalogs WHERE id NOT IN (SELECT MIN(id) FROM catalogs GROUP BY path);

-- Replace non-unique index with unique one
DROP INDEX IF EXISTS idx_catalogs_path;
CREATE UNIQUE INDEX idx_catalogs_path ON catalogs(path);

-- New unique indexes
CREATE UNIQUE INDEX IF NOT EXISTS idx_authors_name_unique ON authors(full_name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_series_name_unique ON series(ser_name);
