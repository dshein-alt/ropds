-- Performance indexes for book browsing and search queries.

-- Covers WHERE avail > 0 filter and ORDER BY reg_date DESC, id DESC in one scan
-- for the "recently added books" page — avoids a full-table sort on every load.
CREATE INDEX idx_books_avail_reg_date ON books(avail, reg_date DESC, id DESC);

-- Covering index for the global hide_doubles dedup subquery:
--   SELECT MAX(id) FROM books WHERE avail > 0 GROUP BY search_title, author_key
CREATE INDEX idx_books_dedup ON books(avail, search_title, author_key, id);

-- Covering index for catalog browsing — covers filter + sort in one scan:
--   WHERE catalog_id = ? AND avail > 0 ORDER BY search_title
-- Also covers the catalog-scoped hide_doubles dedup subquery; id is included
-- explicitly so MIN(id) resolves from the index:
--   SELECT MIN(id) FROM books WHERE catalog_id = ? AND avail > 0
--   GROUP BY search_title, author_key
CREATE INDEX idx_books_catalog_avail_title ON books(catalog_id, avail, search_title, author_key, id);
