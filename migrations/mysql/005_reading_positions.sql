-- Reading positions: per-user/per-book reading progress

CREATE TABLE IF NOT EXISTS reading_positions (
    id         BIGINT       PRIMARY KEY AUTO_INCREMENT,
    user_id    BIGINT       NOT NULL,
    book_id    BIGINT       NOT NULL,
    position   VARCHAR(512) NOT NULL DEFAULT '',
    progress   DOUBLE       NOT NULL DEFAULT 0.0,
    updated_at VARCHAR(64)  NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    UNIQUE(user_id, book_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_rp_user_updated ON reading_positions(user_id, updated_at);
