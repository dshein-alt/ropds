# Руководство по миграции: SQLite -> PostgreSQL / MySQL (MariaDB)

В этой директории лежит скрипт `migrate_sqlite.py`, который переносит все данные ROPDS из SQLite в PostgreSQL или MySQL/MariaDB.

## Что делает скрипт

- Читает все таблицы SQLite (по умолчанию исключая `_sqlx_migrations`)
- Проверяет схему и версии миграций в целевой базе
- Очищает целевые таблицы перед импортом (поведение по умолчанию)
- Переносит данные в порядке, учитывающем зависимости между таблицами
- Сверяет количество строк по каждой таблице после импорта

Скрипт написан на чистом Python (только стандартная библиотека). Для работы с целевой базой запускает `psql` или `mysql`/`mariadb`.

## Что нужно заранее

1. Есть файл исходной базы SQLite (например, `devel/ropds.db`).
2. Целевой сервер БД доступен по сети.
3. В целевой базе уже создана схема ROPDS (применены миграции).

Чтобы инициализировать схему, один раз запустите ROPDS с целевым URL БД в `config.toml`:

```bash
./target/debug/ropds -c /path/to/config.toml --set-admin ваш-пароль
```

## 1) PostgreSQL (CLI на хосте)

1. Остановите ROPDS (на время миграции это рекомендуется).
2. Сделайте резервную копию SQLite:

```bash
cp /path/to/ropds.db /path/to/ropds.db.bak.$(date +%F-%H%M%S)
```

3. Запустите миграцию:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds'
```

4. Запустите ROPDS с URL PostgreSQL в конфиге.

## 2) MySQL / MariaDB (CLI на хосте)

1. Остановите ROPDS.
2. Сделайте резервную копию SQLite.
3. Запустите миграцию:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds'
```

4. Запустите ROPDS с URL MySQL в конфиге.

## 3) Если база в контейнере (и CLI нет на хосте)

Если `psql` или `mysql` не установлены на хосте, можно направить скрипт прямо в контейнер:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'postgres://ropds:secret@127.0.0.1:5432/ropds' \
  --db-container ropds-postgres \
  --container-runtime docker
```

Пример для MariaDB:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds' \
  --db-container ropds-mariadb \
  --container-runtime docker
```

Для Podman:

```bash
--container-runtime podman
```

## Проверка после миграции

1. Запустите ROPDS с конфигом для целевой базы.
2. Откройте `/health` и `/web` — убедитесь, что оба отвечают.
3. Проверьте несколько счётчиков:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

4. Откройте встроенную читалку и убедитесь:
- `POST /web/api/reading-position` возвращает `{"ok": true}`
- `/web/api/reading-history` содержит ожидаемые записи

## Дополнительные параметры

- `--no-truncate-target` — не очищать целевые таблицы перед импортом (для продвинутых сценариев)
- `--include-sqlx-migrations` — перенести также таблицу `_sqlx_migrations`
- `--fetch-batch-size N` — размер порции при чтении из SQLite (по умолчанию `500`)
- `--max-statement-bytes N` — максимальный размер генерируемого SQL-запроса
- `--progress-every-rows N` — интервал вывода прогресса
- `--log-level DEBUG|INFO|WARNING|ERROR`

Полный список:

```bash
python3 scripts/migrate_sqlite.py --help
```

## Замечания

- Несовпадение схемы или версий миграций в целевой базе приводит к ошибке — это сделано намеренно.
- По умолчанию скрипт работает в режиме «безопасной перезаливки»: очистка таблиц, полный импорт, сверка количества строк.
- Убедитесь, что `library.root_path` в новом конфиге совпадает с путями книг, записанными в базе данных.
