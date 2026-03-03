CREATE TABLE IF NOT EXISTS suppressed_books (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    path VARCHAR(2048) NOT NULL,
    filename VARCHAR(255) NOT NULL,
    suppressed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    suppressed_key CHAR(64) NOT NULL,
    UNIQUE KEY uq_suppressed_key (suppressed_key),
    KEY idx_suppressed_path_file (path(255), filename(255))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
