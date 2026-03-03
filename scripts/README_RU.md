# Руководство по миграции SQLite -> PostgreSQL / MySQL(MariaDB)

В этой папке находится `migrate_sqlite.py` для полной миграции данных ROPDS из SQLite в PostgreSQL или MySQL/MariaDB.

## Что Делает Скрипт

- Читает все таблицы SQLite (кроме `_sqlx_migrations` по умолчанию).
- Проверяет схему и версии миграций в целевой БД.
- Очищает таблицы целевой БД перед импортом (поведение по умолчанию).
- Импортирует данные с учетом зависимостей таблиц.
- Проверяет совпадение количества строк после импорта.

Скрипт использует только Python stdlib.  
Для работы с целевой БД использует CLI `psql` или `mysql`/`mariadb`.

## Требования

1. Существует файл исходной SQLite БД (например `devel/ropds.db`).
2. Целевая БД доступна по сети.
3. В целевой БД уже применены миграции ROPDS.

Чтобы инициализировать схему в целевой БД, один раз запустите ROPDS с новым URL:

```bash
./target/debug/ropds -c /path/to/config.toml --set-admin ваш-пароль
```

## 1) Миграция в PostgreSQL (CLI на хосте)

1. Остановите запущенный ROPDS (рекомендуется на время миграции).
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

## 2) Миграция в MariaDB/MySQL (CLI на хосте)

1. Остановите запущенный ROPDS.
2. Сделайте бэкап SQLite.
3. Запустите миграцию:

```bash
python3 scripts/migrate_sqlite.py \
  --sqlite-db /path/to/ropds.db \
  --target-url 'mysql://ropds:secret@127.0.0.1:3306/ropds'
```

4. Запустите ROPDS с URL MySQL в конфиге.

## 3) Если БД Работает В Контейнере (и CLI нет на хосте)

Используйте прямой контейнерный режим:

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

## Проверка После Миграции

После миграции:

1. Запустите ROPDS с целевой БД.
2. Проверьте `/health` и `/web`.
3. Проверьте счетчики:

```sql
SELECT COUNT(*) FROM books WHERE avail = 2;
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM reading_positions;
```

4. Откройте встроенную читалку и убедитесь:
- `POST /web/api/reading-position` возвращает `{"ok": true}`
- `/web/api/reading-history` содержит ожидаемые записи.

## Полезные Опции

- `--no-truncate-target` - не очищать целевые таблицы (для продвинутых сценариев).
- `--include-sqlx-migrations` - переносить и `_sqlx_migrations`.
- `--fetch-batch-size N` - размер выборки из SQLite (по умолчанию `500`).
- `--max-statement-bytes N` - максимум размера SQL-выражения.
- `--progress-every-rows N` - интервал логирования прогресса.
- `--log-level DEBUG|INFO|WARNING|ERROR`

Все параметры:

```bash
python3 scripts/migrate_sqlite.py --help
```

## Примечания

- Несовпадение схемы/миграций в целевой БД вызывает ошибку (это ожидаемо).
- По умолчанию скрипт делает "полную перезаливку" данных (truncate + полный импорт + проверка строк).
- `library.root_path` в новом конфиге должен соответствовать путям книг в БД.
