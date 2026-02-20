-- SOPDS: initial schema (SQLite)

-- Genre sections (language-independent)
CREATE TABLE IF NOT EXISTS genre_sections (
    id   INTEGER PRIMARY KEY,
    code TEXT    NOT NULL UNIQUE
);

-- Genre section translations
CREATE TABLE IF NOT EXISTS genre_section_translations (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    section_id INTEGER NOT NULL REFERENCES genre_sections(id) ON DELETE CASCADE,
    lang       TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    UNIQUE(section_id, lang)
);
CREATE INDEX idx_gst_lang ON genre_section_translations(lang);

-- Catalogs (filesystem directories and archives)
CREATE TABLE IF NOT EXISTS catalogs (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER REFERENCES catalogs(id) ON DELETE CASCADE,
    path      TEXT    NOT NULL DEFAULT '',
    cat_name  TEXT    NOT NULL DEFAULT '',
    cat_type  INTEGER NOT NULL DEFAULT 0,  -- 0=normal, 1=zip, 2=inpx, 3=inp
    cat_size  INTEGER NOT NULL DEFAULT 0,
    cat_mtime TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX        idx_catalogs_parent ON catalogs(parent_id);
CREATE UNIQUE INDEX idx_catalogs_path   ON catalogs(path);

-- Books
CREATE TABLE IF NOT EXISTS books (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    catalog_id   INTEGER NOT NULL REFERENCES catalogs(id) ON DELETE CASCADE,
    filename     TEXT    NOT NULL DEFAULT '',
    path         TEXT    NOT NULL DEFAULT '',
    format       TEXT    NOT NULL DEFAULT '',
    title        TEXT    NOT NULL DEFAULT '',
    search_title TEXT    NOT NULL DEFAULT '',
    annotation   TEXT    NOT NULL DEFAULT '',
    docdate      TEXT    NOT NULL DEFAULT '',
    lang         TEXT    NOT NULL DEFAULT 'un',
    lang_code    INTEGER NOT NULL DEFAULT 9,  -- 1=Cyrillic, 2=Latin, 3=Digit, 9=Other
    size         INTEGER NOT NULL DEFAULT 0,
    avail        INTEGER NOT NULL DEFAULT 1,  -- 0=deleted, 1=unverified, 2=confirmed
    cat_type     INTEGER NOT NULL DEFAULT 0,
    cover        INTEGER NOT NULL DEFAULT 0,
    cover_type   TEXT    NOT NULL DEFAULT '',
    reg_date     TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_books_catalog   ON books(catalog_id);
CREATE INDEX idx_books_search    ON books(search_title);
CREATE INDEX idx_books_lang_code ON books(lang_code);
CREATE INDEX idx_books_avail     ON books(avail);
CREATE INDEX idx_books_format    ON books(format);
CREATE INDEX idx_books_path_file ON books(path, filename);

-- Authors
CREATE TABLE IF NOT EXISTS authors (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name        TEXT    NOT NULL DEFAULT '',
    search_full_name TEXT    NOT NULL DEFAULT '',
    lang_code        INTEGER NOT NULL DEFAULT 9
);
CREATE INDEX        idx_authors_search      ON authors(search_full_name);
CREATE INDEX        idx_authors_lang_code   ON authors(lang_code);
CREATE UNIQUE INDEX idx_authors_name_unique ON authors(full_name);

-- Genres
CREATE TABLE IF NOT EXISTS genres (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    code       TEXT    NOT NULL UNIQUE,
    section    TEXT    NOT NULL DEFAULT '',
    subsection TEXT    NOT NULL DEFAULT '',
    section_id INTEGER REFERENCES genre_sections(id)
);
CREATE INDEX idx_genres_code    ON genres(code);
CREATE INDEX idx_genres_section ON genres(section);

-- Genre translations
CREATE TABLE IF NOT EXISTS genre_translations (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    genre_id INTEGER NOT NULL REFERENCES genres(id) ON DELETE CASCADE,
    lang     TEXT    NOT NULL,
    name     TEXT    NOT NULL,
    UNIQUE(genre_id, lang)
);
CREATE INDEX idx_gt_lang ON genre_translations(lang);

-- Series
CREATE TABLE IF NOT EXISTS series (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    ser_name   TEXT    NOT NULL DEFAULT '',
    search_ser TEXT    NOT NULL DEFAULT '',
    lang_code  INTEGER NOT NULL DEFAULT 9
);
CREATE INDEX        idx_series_search      ON series(search_ser);
CREATE INDEX        idx_series_lang_code   ON series(lang_code);
CREATE UNIQUE INDEX idx_series_name_unique ON series(ser_name);

-- Junction: book <-> author
CREATE TABLE IF NOT EXISTS book_authors (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_book_authors_unique ON book_authors(book_id, author_id);
CREATE INDEX        idx_book_authors_author ON book_authors(author_id);

-- Junction: book <-> genre
CREATE TABLE IF NOT EXISTS book_genres (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    book_id  INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    genre_id INTEGER NOT NULL REFERENCES genres(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_book_genres_unique ON book_genres(book_id, genre_id);
CREATE INDEX        idx_book_genres_genre  ON book_genres(genre_id);

-- Junction: book <-> series
CREATE TABLE IF NOT EXISTS book_series (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    series_id INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    ser_no    INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_book_series_unique ON book_series(book_id, series_id);
CREATE INDEX        idx_book_series_series ON book_series(series_id);

-- Users
CREATE TABLE IF NOT EXISTS users (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    username                 TEXT    NOT NULL UNIQUE,
    password_hash            TEXT    NOT NULL DEFAULT '',
    is_superuser             INTEGER NOT NULL DEFAULT 0,
    created_at               TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_login               TEXT    NOT NULL DEFAULT '',
    password_change_required INTEGER NOT NULL DEFAULT 0,
    display_name             TEXT    NOT NULL DEFAULT '',
    allow_upload             INTEGER NOT NULL DEFAULT 0
);

-- Bookshelf (user's reading list)
CREATE TABLE IF NOT EXISTS bookshelf (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id   INTEGER NOT NULL,
    book_id   INTEGER NOT NULL,
    read_time TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, book_id)
);
CREATE INDEX idx_bookshelf_user ON bookshelf(user_id, read_time);

-- Counters (aggregate statistics)
CREATE TABLE IF NOT EXISTS counters (
    name       TEXT    PRIMARY KEY,
    value      INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO counters (name, value) VALUES ('allbooks', 0);
INSERT INTO counters (name, value) VALUES ('allcatalogs', 0);
INSERT INTO counters (name, value) VALUES ('allauthors', 0);
INSERT INTO counters (name, value) VALUES ('allgenres', 0);
INSERT INTO counters (name, value) VALUES ('allseries', 0);
