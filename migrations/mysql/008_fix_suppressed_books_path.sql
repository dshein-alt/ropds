CREATE TABLE IF NOT EXISTS suppressed_books_v2 (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    path VARCHAR(2048) NOT NULL,
    filename VARCHAR(255) NOT NULL,
    suppressed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    suppressed_key CHAR(64) NOT NULL,
    UNIQUE KEY uq_suppressed_key (suppressed_key),
    KEY idx_suppressed_path_file (path(255), filename(255))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

INSERT IGNORE INTO suppressed_books_v2 (path, filename, suppressed_at, suppressed_key)
SELECT
    path,
    LEFT(filename, 255),
    suppressed_at,
    SHA2(CONCAT(path, '\0', LEFT(filename, 255)), 256)
FROM suppressed_books;

DROP TABLE suppressed_books;
RENAME TABLE suppressed_books_v2 TO suppressed_books;
