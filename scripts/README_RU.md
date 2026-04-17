# Руководство по миграции: SQLite -> PostgreSQL или MySQL/MariaDB

Перенос существующей SQLite-базы ROPDS в PostgreSQL, MySQL или MariaDB выполняется в четыре шага:

1. **Создать роль и БД** на целевом сервере (один раз).
2. **Инициализировать схему через `ropds --init-db`**.
3. **Перенести данные скриптом `scripts/migrate_sqlite.py`**.
4. **Запустить ROPDS против новой БД.**

Скрипт намеренно узкий — он только копирует строки. Создание схемы, служебные записи миграций и предварительные проверки безопасности лежат на стороне ROPDS.

---

## 1. Создание роли и БД

### PostgreSQL

Подключитесь под ролью с правами `CREATEROLE` и `CREATEDB` (обычно `postgres`) и выполните:

```sql
-- Подставьте настоящий пароль вместо 'strongpassword'.
CREATE USER ropds WITH LOGIN PASSWORD 'strongpassword';

CREATE DATABASE ropds
    OWNER ropds
    ENCODING 'UTF8'
    TEMPLATE template0
    LC_COLLATE 'C.UTF-8'
    LC_CTYPE  'C.UTF-8';

\c ropds
GRANT ALL ON SCHEMA public TO ropds;
ALTER SCHEMA public OWNER TO ropds;
```

Для удалённого подключения добавьте запись в `pg_hba.conf` и перечитайте конфигурацию:

```
host    ropds    ropds    10.0.0.0/24    scram-sha-256
```

### MySQL / MariaDB

Подключитесь под `root` и выполните:

```sql
CREATE DATABASE ropds CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- '%' позволяет подключаться с любого хоста; при необходимости ограничьте.
CREATE USER 'ropds'@'%' IDENTIFIED BY 'strongpassword';
GRANT ALL PRIVILEGES ON ropds.* TO 'ropds'@'%';
FLUSH PRIVILEGES;
```

**Для миграции роль не обязана быть суперпользователем PostgreSQL или `root` в MySQL.** Достаточно того, что она владеет целевой БД.

---

## 2. Подготовка схемы через `ropds --init-db`

Укажите в конфиге ROPDS `[database].url` на целевую БД и выполните:

```bash
cargo run -- --config=config.toml --init-db
# или, при наличии собранного бинарника:
./target/release/ropds --config=config.toml --init-db
```

Что делает команда:

- Подключается к целевой БД по URL из конфига. Если БД не существует и у роли есть права, создаёт её.
- Предварительная проверка: если в целевой БД уже есть таблицы ROPDS, но нет `_sqlx_migrations`, команда откажется работать и завершится с ненулевым кодом.
- Применяет все встроенные миграции по порядку (идемпотентно — уже применённые пропускаются).
- **Очищает все пользовательские таблицы**, чтобы целевая БД действительно была пустой. Seed-строки, которые миграции обычно вставляют (жанры, счётчики и т. д.), стираются — они будут восстановлены из SQLite-источника на шаге 3.
- Завершается кодом 0.

После этого в целевой БД есть полная схема, таблица `_sqlx_migrations` с корректными контрольными суммами под конкретный backend и ноль строк данных.

> **`--init-db` — это подготовка к миграции, а не «fresh install».** Для чистой установки без источника SQLite просто запустите сервер как обычно; в этом режиме миграции применяются и seed-данные остаются на месте.
>
> **Защита**: если в какой-либо таблице данных уже есть строки, `--init-db` отказывается работать и выходит с ошибкой, перечисляя заполненные таблицы и то, что нужно сделать вручную — очистить эти таблицы (или удалить и пересоздать БД) перед повторным запуском. Таким образом `--init-db` безопасно случайно запустить на живой или уже наполненной миграцией БД.

---

## 3. Копирование данных

### Клиент на хосте

```bash
# PostgreSQL
python3 scripts/migrate_sqlite.py \
    /path/to/ropds.db \
    'postgres://ropds:strongpassword@db.example.com:5432/ropds'

# MySQL / MariaDB
python3 scripts/migrate_sqlite.py \
    /path/to/ropds.db \
    'mysql://ropds:strongpassword@db.example.com:3306/ropds'
```

Бэкенд определяется схемой URL (`postgres://` / `postgresql://` или `mysql://` / `mariadb://`).

### БД запущена в контейнере

Если целевая БД работает в контейнере, а на хосте нет `psql`/`mysql`, направьте клиент через контейнер:

```bash
# docker
python3 scripts/migrate_sqlite.py \
    --db-container ropds-postgres --container-runtime docker \
    /path/to/ropds.db \
    'postgres://ropds:strongpassword@127.0.0.1:5432/ropds'

# podman
python3 scripts/migrate_sqlite.py \
    --db-container ropds-mariadb --container-runtime podman \
    /path/to/ropds.db \
    'mysql://ropds:strongpassword@127.0.0.1:3306/ropds'
```

При `--db-container` скрипт запускает `<runtime> exec -i <container> psql|mysql …`, поэтому host/port в URL — это адрес, на котором БД слушает **внутри контейнера** (обычно 5432 / 3306), а не проброшенный порт на хосте.

### Что делает скрипт

1. Открывает SQLite в режиме read-only.
2. Проверяет, что для каждой таблицы-источника в целевой БД есть такая же таблица, а каждая колонка источника присутствует в целевой схеме.
3. **Проверка «пустоты»**: каждая таблица данных в целевой БД должна иметь 0 строк (это состояние, которое оставляет `ropds --init-db`). Если в какой-либо таблице есть строки, скрипт перечислит виновников и откажется продолжать — так нельзя молча затирать уже наполненную БД.
4. Спрашивает `Proceed? [y/N]:`. Ответьте `y`, чтобы продолжить. Piped stdin запрещён, чтобы нельзя было молча запустить деструктивную операцию.
5. Генерирует один SQL-скрипт с truncation + пакетными INSERT'ами (строки в таблицах с self-reference упорядочены «родитель раньше ребёнка») + сбросом автоинкрементов, и выполняет его в одной CLI-сессии:
   - **PostgreSQL**: `BEGIN; TRUNCATE … RESTART IDENTITY CASCADE; INSERT …; SELECT setval(…); COMMIT;` — атомарно; при ошибке транзакция откатывается.
   - **MySQL/MariaDB**: `SET FOREIGN_KEY_CHECKS = 0; TRUNCATE TABLE … ; INSERT …; ALTER TABLE … AUTO_INCREMENT = …; SET FOREIGN_KEY_CHECKS = 1;` — учтите, что MySQL `TRUNCATE` auto-commit'ится, поэтому операция **не** атомарна; при ошибке часть данных может быть уже загружена, и нужно запустить скрипт повторно.
6. Сверяет количество строк по каждой таблице с источником и выводит итог.

Поскольку всё деструктивное делается в одной CLI-сессии, `SET FOREIGN_KEY_CHECKS = 0` / транзакционные настройки применяются ко всем операциям — нет проблемы «сессии разные, настройка не действует».

---

## 4. Запуск ROPDS

```bash
./target/release/ropds --config=config.toml
```

`sqlx` видит, что все миграции уже записаны с корректными контрольными суммами, данные на месте, сервер слушает порт.

---

## Резервная копия перед миграцией

```bash
cp /path/to/ropds.db /path/to/ropds.db.bak.$(date +%F-%H%M%S)
```

## Проверка после миграции

1. Откройте `/health` и `/web` — оба должны отвечать.
2. Сверьте несколько счётчиков:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

3. Откройте встроенную читалку: `POST /web/api/reading-position` должен возвращать `{"ok": true}`, `/web/api/reading-history` — ожидаемые записи.

## Замечания

- `_sqlx_migrations` скрипт никогда не трогает; её ведёт `ropds --init-db`.
- Скрипт идемпотентен, если между попытками запускать `ropds --init-db`: init-db очищает целевую БД, проверка «пустоты» проходит, скрипт снова копирует данные. Второй запуск скрипта без повторной init-db провалит проверку (первый запуск уже наполнил БД).
- Несоответствие схем (в источнике есть колонка, которой нет в целевой) приводит к ошибке — это намеренно.
- Убедитесь, что `library.root_path` в новом конфиге совпадает с путями книг, записанными в БД.
