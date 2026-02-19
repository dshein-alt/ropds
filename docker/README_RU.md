# Развёртывание ROPDS с помощью Docker

В этой директории находится готовый комплект для развёртывания ROPDS в Docker, включающий базовый и дополнительные (override) файлы compose.

## Предварительные требования

- Установленные Docker Engine и Docker Compose v2
- Папка библиотеки на хост-машине (для книг, обложек и загрузок)

## Быстрый старт (SQLite — вариант по умолчанию)

1. Создайте файл с переменными окружения:

```bash
cp docker/.env.example docker/.env
```

2. Запустите ROPDS с SQLite, используя выделенный том Docker:

```bash
docker compose \
  -f docker/docker-compose.yml \
  -f docker/docker-compose.sqlite.yml \
  up -d --build
```

3. Откройте интерфейс в браузере:

- Веб-интерфейс: `http://localhost:8081/web`
- OPDS-каталог: `http://localhost:8081/opds`

## Матрица файлов Compose

| Сценарий | Команда |
|---|---|
| SQLite (БД на томе) | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.sqlite.yml up -d --build` |
| PostgreSQL в соседнем стеке | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.postgres.sibling.yml up -d --build` |
| Внешний сервер PostgreSQL | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.postgres.external.yml up -d --build` |
| MySQL/MariaDB в соседнем стеке | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.mysql.sibling.yml up -d --build` |
| Внешний сервер MySQL/MariaDB | `docker compose -f docker/docker-compose.yml -f docker/docker-compose.mysql.external.yml up -d --build` |

## Соседние стеки баз данных

Чтобы запустить стек PostgreSQL:

```bash
docker compose -f docker/db/postgres/docker-compose.yml up -d
```

Чтобы запустить стек MariaDB:

```bash
docker compose -f docker/db/mysql/docker-compose.yml up -d
```

После этого запустите ROPDS с соответствующим файлом переопределения `*.sibling.yml`.

## Конфигурационные файлы

Все конфигурации приложения находятся в директории `docker/config/`:

- `config.toml.example` — готовая конфигурация для Docker (SQLite), используется в варианте с SQLite
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

Учётные данные и параметры подключения можно редактировать прямо в этих файлах.

## Модель монтирования

В базовом compose-файле выполняются следующие монтирования:

- `./config/*.toml -> /app/config/config.toml` (только чтение)
- `${ROPDS_LIBRARY_ROOT} -> /library` (чтение и запись)

Docker-образ полностью автономен и уже содержит:

- `/app/templates`
- `/app/locales`
- `/app/static`

При необходимости можно смонтировать собственные статические файлы с хоста (в режиме только чтение):

```bash
docker compose \
  -f docker/docker-compose.yml \
  -f docker/docker-compose.sqlite.yml \
  -f docker/docker-compose.static.mount.yml \
  up -d --build
```

Во время работы создаются и используются:

- `/library/covers`
- `/library/uploads`
- Том данных SQLite по пути `/var/lib/ropds/sqlite` (в случае SQLite)

## Первичная настройка администратора

Точка входа (entrypoint) поддерживает автоматическую инициализацию учётной записи администратора:

- В файле `docker/.env` необходимо задать `ROPDS_ADMIN_PASSWORD`.
- При установленном `ROPDS_ADMIN_INIT_ONCE=true` (по умолчанию):
  - выполняется команда `ropds --set-admin ...` один раз;
  - создаётся файл-маркер `/library/.ropds_admin_initialized`.

Чтобы пароль администратора обновлялся при каждом запуске, установите `ROPDS_ADMIN_INIT_ONCE=false`.

## Ожидание базы данных и миграции

Если используется PostgreSQL или MySQL, точка входа ожидает доступности порта БД перед запуском приложения.
Цель ожидания можно задать вручную через переменные `ROPDS_DB_HOST` и `ROPDS_DB_PORT`.

Миграции выполняются автоматически при старте, в зависимости от выбранного типа базы данных.

## Замечания по безопасности

- Обязательно измените `ROPDS_ADMIN_PASSWORD` и `session_secret` перед использованием в продакшене.
- В продакшене для `ROPDS_LIBRARY_ROOT` используйте абсолютный путь.
- Конфигурационный файл должен быть в режиме только чтение.
- Не меняйте `session_secret` между перезапусками, чтобы сохранить активные сессии пользователей.
- Так как `ROPDS_ADMIN_PASSWORD` передаётся в виде аргумента командной строки, он может быть виден в списке процессов (`/proc/PID/cmdline`, `docker inspect`). Для повышенной безопасности можно выполнить `ropds --set-admin` вручную внутри контейнера.

## Организация библиотеки

- По умолчанию `covers_path` и `upload_path` указывают внутрь `/library`.
- В каталогах с книгами храните только поддерживаемые форматы, чтобы избежать сканирования лишних файлов.
