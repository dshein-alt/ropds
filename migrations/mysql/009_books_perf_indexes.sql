-- Performance indexes for book browsing and search queries.
-- Requires MySQL 8.0+ for DESC column direction support.

-- Covers WHERE avail > 0 filter and ORDER BY reg_date DESC, id DESC in one scan
-- for the "recently added books" page — avoids a full-table sort on every load.
CREATE INDEX idx_books_avail_reg_date ON books(avail, reg_date DESC, id DESC);

-- Covering index for the global hide_doubles dedup subquery (InnoDB secondary
-- indexes implicitly include the PK, so id is available without explicit inclusion):
--   SELECT MAX(id) FROM books WHERE avail > 0 GROUP BY search_title, author_key
CREATE INDEX idx_books_dedup ON books(avail, search_title(255), author_key(255), id);

-- Covering index for catalog browsing — covers filter + sort in one scan:
--   WHERE catalog_id = ? AND avail > 0 ORDER BY search_title
-- Also covers the catalog-scoped hide_doubles dedup subquery:
--   SELECT MIN(id) FROM books WHERE catalog_id = ? AND avail > 0
--   GROUP BY search_title, author_key
CREATE INDEX idx_books_catalog_avail_title ON books(catalog_id, avail, search_title(255), author_key(255));
