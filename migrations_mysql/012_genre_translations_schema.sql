-- Genre translations: normalized section/genre structure with per-language names.
--
-- genre_sections       -- language-independent section definitions (22 rows)
-- genre_section_translations -- section display names per language
-- genre_translations   -- genre (subsection) display names per language
-- genres.section_id    -- FK linking each genre to its section

-- 1. Create genre_sections table
CREATE TABLE IF NOT EXISTS genre_sections (
    id   BIGINT PRIMARY KEY AUTO_INCREMENT,
    code VARCHAR(255) NOT NULL UNIQUE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 2. Create genre_section_translations table
CREATE TABLE IF NOT EXISTS genre_section_translations (
    id         BIGINT PRIMARY KEY AUTO_INCREMENT,
    section_id BIGINT NOT NULL,
    lang       VARCHAR(16) NOT NULL,
    name       VARCHAR(512) NOT NULL,
    UNIQUE(section_id, lang),
    FOREIGN KEY (section_id) REFERENCES genre_sections(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX IF NOT EXISTS idx_gst_lang ON genre_section_translations(lang);

-- 3. Create genre_translations table
CREATE TABLE IF NOT EXISTS genre_translations (
    id       BIGINT PRIMARY KEY AUTO_INCREMENT,
    genre_id BIGINT NOT NULL,
    lang     VARCHAR(16) NOT NULL,
    name     VARCHAR(512) NOT NULL,
    UNIQUE(genre_id, lang),
    FOREIGN KEY (genre_id) REFERENCES genres(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
CREATE INDEX IF NOT EXISTS idx_gt_lang ON genre_translations(lang);

-- 4. Add section_id column to genres
ALTER TABLE genres ADD COLUMN section_id BIGINT;

-- 5. Populate genre_sections with hardcoded IDs
INSERT INTO genre_sections (id, code) VALUES
    (1,  'business'),
    (2,  'detective'),
    (3,  'nonfiction'),
    (4,  'home_family'),
    (5,  'drama'),
    (6,  'art'),
    (7,  'computers'),
    (8,  'children'),
    (9,  'romance'),
    (10, 'science'),
    (11, 'poetry'),
    (12, 'adventure'),
    (13, 'prose'),
    (14, 'other'),
    (15, 'religion'),
    (16, 'reference'),
    (17, 'antique'),
    (18, 'tech'),
    (19, 'textbooks'),
    (20, 'sf'),
    (21, 'folklore'),
    (22, 'humor');

-- 6. Fix typo in genre 48: 'Искусттво' -> 'Искусство'
UPDATE genres SET section = 'Искусство, Искусствоведение, Дизайн' WHERE id = 48;

-- 7. Set section_id for all genres based on their current section text
UPDATE genres SET section_id = 1  WHERE section = 'Деловая литература';
UPDATE genres SET section_id = 2  WHERE section = 'Детективы и Триллеры';
UPDATE genres SET section_id = 3  WHERE section = 'Документальная литература';
UPDATE genres SET section_id = 4  WHERE section = 'Дом и семья';
UPDATE genres SET section_id = 5  WHERE section = 'Драматургия';
UPDATE genres SET section_id = 6  WHERE section = 'Искусство, Искусствоведение, Дизайн';
UPDATE genres SET section_id = 7  WHERE section = 'Компьютеры и Интернет';
UPDATE genres SET section_id = 8  WHERE section = 'Литература для детей';
UPDATE genres SET section_id = 9  WHERE section = 'Любовные романы';
UPDATE genres SET section_id = 10 WHERE section = 'Наука, Образование';
UPDATE genres SET section_id = 11 WHERE section = 'Поэзия';
UPDATE genres SET section_id = 12 WHERE section = 'Приключения';
UPDATE genres SET section_id = 13 WHERE section = 'Проза';
UPDATE genres SET section_id = 14 WHERE section = 'Прочее';
UPDATE genres SET section_id = 15 WHERE section = 'Религия, духовность, эзотерика';
UPDATE genres SET section_id = 16 WHERE section = 'Справочная литература';
UPDATE genres SET section_id = 17 WHERE section = 'Старинное';
UPDATE genres SET section_id = 18 WHERE section = 'Техника';
UPDATE genres SET section_id = 19 WHERE section = 'Учебники и пособия';
UPDATE genres SET section_id = 20 WHERE section = 'Фантастика';
UPDATE genres SET section_id = 21 WHERE section = 'Фольклор';
UPDATE genres SET section_id = 22 WHERE section = 'Юмор';
