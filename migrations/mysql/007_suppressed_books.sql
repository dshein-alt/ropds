CREATE TABLE IF NOT EXISTS suppressed_books (
    path VARCHAR(512) NOT NULL,
    filename VARCHAR(512) NOT NULL,
    suppressed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (path, filename)
);
