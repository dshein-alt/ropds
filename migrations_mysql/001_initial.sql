-- SOPDS: initial schema (MySQL/MariaDB)
-- Note: Use VARCHAR instead of TEXT to avoid MariaDB TEXT->BLOB wire-protocol issue.

-- Genre sections (language-independent)
CREATE TABLE IF NOT EXISTS genre_sections (
    id   BIGINT PRIMARY KEY AUTO_INCREMENT,
    code VARCHAR(255) NOT NULL UNIQUE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Genre section translations
CREATE TABLE IF NOT EXISTS genre_section_translations (
    id         BIGINT PRIMARY KEY AUTO_INCREMENT,
    section_id BIGINT       NOT NULL,
    lang       VARCHAR(16)  NOT NULL,
    name       VARCHAR(512) NOT NULL,
    UNIQUE(section_id, lang),
    FOREIGN KEY (section_id) REFERENCES genre_sections(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_gst_lang ON genre_section_translations(lang);

-- Catalogs (filesystem directories and archives)
CREATE TABLE IF NOT EXISTS catalogs (
    id        BIGINT        PRIMARY KEY AUTO_INCREMENT,
    parent_id BIGINT,
    path      VARCHAR(2048) NOT NULL DEFAULT '',
    cat_name  VARCHAR(255)  NOT NULL DEFAULT '',
    cat_type  INTEGER       NOT NULL DEFAULT 0,  -- 0=normal, 1=zip, 2=inpx, 3=inp
    cat_size  INTEGER       NOT NULL DEFAULT 0,
    cat_mtime VARCHAR(64)   NOT NULL DEFAULT '',
    FOREIGN KEY (parent_id) REFERENCES catalogs(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX        idx_catalogs_parent ON catalogs(parent_id);
CREATE UNIQUE INDEX idx_catalogs_path   ON catalogs(path(255));

-- Books
CREATE TABLE IF NOT EXISTS books (
    id           BIGINT        PRIMARY KEY AUTO_INCREMENT,
    catalog_id   BIGINT        NOT NULL,
    filename     VARCHAR(255)  NOT NULL DEFAULT '',
    path         VARCHAR(2048) NOT NULL DEFAULT '',
    format       VARCHAR(64)   NOT NULL DEFAULT '',
    title        VARCHAR(512)  NOT NULL DEFAULT '',
    search_title VARCHAR(512)  NOT NULL DEFAULT '',
    annotation   VARCHAR(8000) NOT NULL DEFAULT '',
    docdate      VARCHAR(64)   NOT NULL DEFAULT '',
    lang         VARCHAR(16)   NOT NULL DEFAULT 'un',
    lang_code    INTEGER       NOT NULL DEFAULT 9,  -- 1=Cyrillic, 2=Latin, 3=Digit, 9=Other
    size         INTEGER       NOT NULL DEFAULT 0,
    avail        INTEGER       NOT NULL DEFAULT 1,  -- 0=deleted, 1=unverified, 2=confirmed
    cat_type     INTEGER       NOT NULL DEFAULT 0,
    cover        INTEGER       NOT NULL DEFAULT 0,
    cover_type   VARCHAR(64)   NOT NULL DEFAULT '',
    reg_date     VARCHAR(64)   NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    FOREIGN KEY (catalog_id) REFERENCES catalogs(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_books_catalog   ON books(catalog_id);
CREATE INDEX idx_books_search    ON books(search_title(255));
CREATE INDEX idx_books_lang_code ON books(lang_code);
CREATE INDEX idx_books_avail     ON books(avail);
CREATE INDEX idx_books_format    ON books(format);
CREATE INDEX idx_books_path_file ON books(path(255), filename(255));

-- Authors
CREATE TABLE IF NOT EXISTS authors (
    id               BIGINT       PRIMARY KEY AUTO_INCREMENT,
    full_name        VARCHAR(512) NOT NULL DEFAULT '',
    search_full_name VARCHAR(512) NOT NULL DEFAULT '',
    lang_code        INTEGER      NOT NULL DEFAULT 9
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX        idx_authors_search      ON authors(search_full_name(255));
CREATE INDEX        idx_authors_lang_code   ON authors(lang_code);
CREATE UNIQUE INDEX idx_authors_name_unique ON authors(full_name(255));

-- Genres
CREATE TABLE IF NOT EXISTS genres (
    id         BIGINT       PRIMARY KEY AUTO_INCREMENT,
    code       VARCHAR(255) NOT NULL UNIQUE,
    section    VARCHAR(512) NOT NULL DEFAULT '',
    subsection VARCHAR(512) NOT NULL DEFAULT '',
    section_id BIGINT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_genres_code    ON genres(code);
CREATE INDEX idx_genres_section ON genres(section(255));

-- Genre translations
CREATE TABLE IF NOT EXISTS genre_translations (
    id       BIGINT       PRIMARY KEY AUTO_INCREMENT,
    genre_id BIGINT       NOT NULL,
    lang     VARCHAR(16)  NOT NULL,
    name     VARCHAR(512) NOT NULL,
    UNIQUE(genre_id, lang),
    FOREIGN KEY (genre_id) REFERENCES genres(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_gt_lang ON genre_translations(lang);

-- Series
CREATE TABLE IF NOT EXISTS series (
    id         BIGINT       PRIMARY KEY AUTO_INCREMENT,
    ser_name   VARCHAR(512) NOT NULL DEFAULT '',
    search_ser VARCHAR(512) NOT NULL DEFAULT '',
    lang_code  INTEGER      NOT NULL DEFAULT 9
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX        idx_series_search      ON series(search_ser(255));
CREATE INDEX        idx_series_lang_code   ON series(lang_code);
CREATE UNIQUE INDEX idx_series_name_unique ON series(ser_name(255));

-- Junction: book <-> author
CREATE TABLE IF NOT EXISTS book_authors (
    id        BIGINT PRIMARY KEY AUTO_INCREMENT,
    book_id   BIGINT NOT NULL,
    author_id BIGINT NOT NULL,
    FOREIGN KEY (book_id) REFERENCES books(id) ON DELETE CASCADE,
    FOREIGN KEY (author_id) REFERENCES authors(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE UNIQUE INDEX idx_book_authors_unique ON book_authors(book_id, author_id);
CREATE INDEX        idx_book_authors_author ON book_authors(author_id);

-- Junction: book <-> genre
CREATE TABLE IF NOT EXISTS book_genres (
    id       BIGINT PRIMARY KEY AUTO_INCREMENT,
    book_id  BIGINT NOT NULL,
    genre_id BIGINT NOT NULL,
    FOREIGN KEY (book_id) REFERENCES books(id) ON DELETE CASCADE,
    FOREIGN KEY (genre_id) REFERENCES genres(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE UNIQUE INDEX idx_book_genres_unique ON book_genres(book_id, genre_id);
CREATE INDEX        idx_book_genres_genre  ON book_genres(genre_id);

-- Junction: book <-> series
CREATE TABLE IF NOT EXISTS book_series (
    id        BIGINT PRIMARY KEY AUTO_INCREMENT,
    book_id   BIGINT  NOT NULL,
    series_id BIGINT  NOT NULL,
    ser_no    INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (book_id) REFERENCES books(id) ON DELETE CASCADE,
    FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE UNIQUE INDEX idx_book_series_unique ON book_series(book_id, series_id);
CREATE INDEX        idx_book_series_series ON book_series(series_id);

-- Users
CREATE TABLE IF NOT EXISTS users (
    id                       BIGINT       PRIMARY KEY AUTO_INCREMENT,
    username                 VARCHAR(255) NOT NULL UNIQUE,
    password_hash            VARCHAR(512) NOT NULL DEFAULT '',
    is_superuser             INTEGER      NOT NULL DEFAULT 0,
    created_at               VARCHAR(64)  NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    last_login               VARCHAR(64)  NOT NULL DEFAULT '',
    password_change_required INTEGER      NOT NULL DEFAULT 0,
    display_name             VARCHAR(255) NOT NULL DEFAULT '',
    allow_upload             INTEGER      NOT NULL DEFAULT 0
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Bookshelf (user's reading list)
CREATE TABLE IF NOT EXISTS bookshelf (
    id        BIGINT      PRIMARY KEY AUTO_INCREMENT,
    user_id   BIGINT      NOT NULL,
    book_id   BIGINT      NOT NULL,
    read_time VARCHAR(64) NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    UNIQUE(user_id, book_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX idx_bookshelf_user ON bookshelf(user_id, read_time);

-- Counters (aggregate statistics)
CREATE TABLE IF NOT EXISTS counters (
    name       VARCHAR(255) PRIMARY KEY,
    value      INTEGER      NOT NULL DEFAULT 0,
    updated_at VARCHAR(64)  NOT NULL DEFAULT (CURRENT_TIMESTAMP)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

INSERT INTO counters (name, value) VALUES ('allbooks', 0);
INSERT INTO counters (name, value) VALUES ('allcatalogs', 0);
INSERT INTO counters (name, value) VALUES ('allauthors', 0);
INSERT INTO counters (name, value) VALUES ('allgenres', 0);
INSERT INTO counters (name, value) VALUES ('allseries', 0);
