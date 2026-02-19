-- Core schema for ropds (Rust OPDS server) — PostgreSQL version

CREATE TABLE IF NOT EXISTS catalogs (
    id          BIGSERIAL PRIMARY KEY,
    parent_id   BIGINT REFERENCES catalogs(id) ON DELETE CASCADE,
    path        TEXT    NOT NULL DEFAULT '',
    cat_name    TEXT    NOT NULL DEFAULT '',
    cat_type    INTEGER NOT NULL DEFAULT 0  -- 0=normal, 1=zip, 2=inpx, 3=inp
);
CREATE INDEX idx_catalogs_parent ON catalogs(parent_id);
CREATE INDEX idx_catalogs_path   ON catalogs(path);

CREATE TABLE IF NOT EXISTS books (
    id              BIGSERIAL PRIMARY KEY,
    catalog_id      BIGINT    NOT NULL REFERENCES catalogs(id) ON DELETE CASCADE,
    filename        TEXT      NOT NULL DEFAULT '',
    path            TEXT      NOT NULL DEFAULT '',
    format          TEXT      NOT NULL DEFAULT '',
    title           TEXT      NOT NULL DEFAULT '',
    search_title    TEXT      NOT NULL DEFAULT '',
    annotation      TEXT      NOT NULL DEFAULT '',
    docdate         TEXT      NOT NULL DEFAULT '',
    lang            TEXT      NOT NULL DEFAULT 'un',
    lang_code       INTEGER   NOT NULL DEFAULT 9,  -- 1=Cyrillic, 2=Latin, 3=Digit, 9=Other
    size            INTEGER   NOT NULL DEFAULT 0,
    avail           INTEGER   NOT NULL DEFAULT 1,  -- 0=deleted, 1=unverified, 2=confirmed
    cat_type        INTEGER   NOT NULL DEFAULT 0,
    cover           INTEGER   NOT NULL DEFAULT 0,
    cover_type      TEXT      NOT NULL DEFAULT '',
    reg_date        TEXT      NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_books_catalog    ON books(catalog_id);
CREATE INDEX idx_books_search     ON books(search_title);
CREATE INDEX idx_books_lang_code  ON books(lang_code);
CREATE INDEX idx_books_avail      ON books(avail);
CREATE INDEX idx_books_format     ON books(format);
CREATE INDEX idx_books_path_file  ON books(path, filename);

CREATE TABLE IF NOT EXISTS authors (
    id                  BIGSERIAL PRIMARY KEY,
    full_name           TEXT    NOT NULL DEFAULT '',
    search_full_name    TEXT    NOT NULL DEFAULT '',
    lang_code           INTEGER NOT NULL DEFAULT 9
);
CREATE INDEX idx_authors_search    ON authors(search_full_name);
CREATE INDEX idx_authors_lang_code ON authors(lang_code);

CREATE TABLE IF NOT EXISTS genres (
    id          BIGSERIAL PRIMARY KEY,
    code        TEXT    NOT NULL UNIQUE,
    section     TEXT    NOT NULL DEFAULT '',
    subsection  TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_genres_code    ON genres(code);
CREATE INDEX idx_genres_section ON genres(section);

CREATE TABLE IF NOT EXISTS series (
    id          BIGSERIAL PRIMARY KEY,
    ser_name    TEXT    NOT NULL DEFAULT '',
    search_ser  TEXT    NOT NULL DEFAULT '',
    lang_code   INTEGER NOT NULL DEFAULT 9
);
CREATE INDEX idx_series_search    ON series(search_ser);
CREATE INDEX idx_series_lang_code ON series(lang_code);

CREATE TABLE IF NOT EXISTS book_authors (
    id          BIGSERIAL PRIMARY KEY,
    book_id     BIGINT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    author_id   BIGINT NOT NULL REFERENCES authors(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_book_authors_unique ON book_authors(book_id, author_id);
CREATE INDEX idx_book_authors_author        ON book_authors(author_id);

CREATE TABLE IF NOT EXISTS book_genres (
    id          BIGSERIAL PRIMARY KEY,
    book_id     BIGINT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    genre_id    BIGINT NOT NULL REFERENCES genres(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_book_genres_unique ON book_genres(book_id, genre_id);
CREATE INDEX idx_book_genres_genre         ON book_genres(genre_id);

CREATE TABLE IF NOT EXISTS book_series (
    id          BIGSERIAL PRIMARY KEY,
    book_id     BIGINT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    series_id   BIGINT NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    ser_no      INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_book_series_unique ON book_series(book_id, series_id);
CREATE INDEX idx_book_series_series        ON book_series(series_id);

CREATE TABLE IF NOT EXISTS users (
    id              BIGSERIAL PRIMARY KEY,
    username        TEXT      NOT NULL UNIQUE,
    password_hash   TEXT      NOT NULL DEFAULT '',
    is_superuser    INTEGER   NOT NULL DEFAULT 0,
    created_at      TEXT      NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS bookshelves (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id     BIGINT    NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    read_time   TEXT      NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX idx_bookshelves_unique ON bookshelves(user_id, book_id);
CREATE INDEX idx_bookshelves_user          ON bookshelves(user_id);

CREATE TABLE IF NOT EXISTS counters (
    name        TEXT      PRIMARY KEY,
    value       INTEGER   NOT NULL DEFAULT 0,
    updated_at  TEXT      NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Initialize default counters
INSERT INTO counters (name, value) VALUES ('allbooks', 0);
INSERT INTO counters (name, value) VALUES ('allcatalogs', 0);
INSERT INTO counters (name, value) VALUES ('allauthors', 0);
INSERT INTO counters (name, value) VALUES ('allgenres', 0);
INSERT INTO counters (name, value) VALUES ('allseries', 0);
