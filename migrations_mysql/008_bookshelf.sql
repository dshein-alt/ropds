CREATE TABLE IF NOT EXISTS bookshelf (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    user_id BIGINT NOT NULL,
    book_id BIGINT NOT NULL,
    read_time TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    UNIQUE(user_id, book_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX IF NOT EXISTS idx_bookshelf_user ON bookshelf(user_id, read_time(255));
