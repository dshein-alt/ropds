-- Add author_key to books for duplicate detection

ALTER TABLE books ADD COLUMN author_key TEXT NOT NULL DEFAULT '';

-- Backfill author_key from existing book_authors links
UPDATE books SET author_key = COALESCE(
    (SELECT GROUP_CONCAT(author_id, ',')
     FROM (SELECT author_id FROM book_authors WHERE book_id = books.id ORDER BY author_id)
    ), ''
) WHERE EXISTS (SELECT 1 FROM book_authors WHERE book_id = books.id);

CREATE INDEX idx_books_author_key ON books(search_title, author_key);
