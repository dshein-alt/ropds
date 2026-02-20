-- Reading positions: per-user/per-book reading progress

CREATE TABLE IF NOT EXISTS reading_positions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id    INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    position   TEXT    NOT NULL DEFAULT '',
    progress   REAL    NOT NULL DEFAULT 0.0,
    updated_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, book_id)
);
CREATE INDEX idx_rp_user_updated ON reading_positions(user_id, updated_at);
