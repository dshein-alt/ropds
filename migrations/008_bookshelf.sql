CREATE TABLE IF NOT EXISTS bookshelf (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    read_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, book_id)
);
CREATE INDEX IF NOT EXISTS idx_bookshelf_user ON bookshelf(user_id, read_time);
