CREATE TABLE IF NOT EXISTS suppressed_books (
    path TEXT NOT NULL,
    filename TEXT NOT NULL,
    suppressed_at TEXT NOT NULL DEFAULT (now()),
    PRIMARY KEY (path, filename)
);
