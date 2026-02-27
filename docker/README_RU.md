# Развёртывание ROPDS с помощью Docker

В этой директории находится готовый комплект для развёртывания ROPDS в Docker с отдельными compose-файлами для каждого сценария.

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
docker compose -f docker/docker-compose.sqlite.yml up -d --build
```

3. Откройте интерфейс в браузере:

- Веб-интерфейс: `http://localhost:8081/web`
- OPDS-каталог: `http://localhost:8081/opds`

## Матрица файлов Compose

Каждый сценарий — это один самодостаточный compose-файл.

| Сценарий | Команда |
|---|---|
| SQLite (БД на томе) | `docker compose -f docker/docker-compose.sqlite.yml up -d --build` |
| PostgreSQL (встроенный) | `docker compose -f docker/docker-compose.postgres.sibling.yml up -d --build` |
| Внешний сервер PostgreSQL | `docker compose -f docker/docker-compose.postgres.external.yml up -d --build` |
| MySQL/MariaDB (встроенный) | `docker compose -f docker/docker-compose.mysql.sibling.yml up -d --build` |
| Внешний сервер MySQL/MariaDB | `docker compose -f docker/docker-compose.mysql.external.yml up -d --build` |

**Встроенные** сценарии включают и ROPDS, и сервис базы данных в одном compose-файле — одна команда `docker compose up` запускает всё.

**Внешние** сценарии запускают только ROPDS и подключаются к базе данных, размещённой отдельно.

## Конфигурационные файлы

Все конфигурации приложения находятся в директории `docker/config/`:

- `config.toml.example` — готовая конфигурация для Docker (SQLite), используется в варианте с SQLite
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

Учётные данные и параметры подключения можно редактировать прямо в этих файлах.

## Модель монтирования

Каждый compose-файл монтирует:

- `./config/*.toml -> /app/config/config.toml` (только чтение)
- `${ROPDS_LIBRARY_ROOT} -> /library` (чтение и запись)

Веб-шаблоны, статические файлы и локали встраиваются в release-бинарник на этапе сборки.

Во время работы создаются и используются:

- `/library/covers`
- `/library/uploads`
- Том данных SQLite по пути `/var/lib/ropds/sqlite` (в случае SQLite)

## Переменные окружения

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `TZ` | (нет) | Часовой пояс контейнера (напр. `Europe/Moscow`) |
| `ROPDS_PORT` | `8081` | Порт HTTP на хосте |
| `ROPDS_LIBRARY_ROOT` | `../library` | Путь к библиотеке на хосте |
| `ROPDS_ADMIN_PASSWORD` | (нет) | Пароль администратора при первом запуске |
| `ROPDS_ADMIN_INIT_ONCE` | `true` | Однократная инициализация администратора |
| `ROPDS_DB_WAIT_TIMEOUT` | `60` | Тайм-аут ожидания БД (секунды) |
| `ROPDS_DB_HOST` | (нет) | Явное указание хоста БД |
| `ROPDS_DB_PORT` | (нет) | Явное указание порта БД |

Для встроенных сценариев с БД (PostgreSQL и MySQL/MariaDB):

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `DB_NAME` | `ropds` | Имя базы данных |
| `DB_USER` | `ropds` | Пользователь БД |
| `DB_PASSWORD` | `ropds_change_me` | Пароль пользователя БД |
| `DB_ROOT_PASSWORD` | `root_change_me` | Пароль root MariaDB (только MySQL) |

Значения должны совпадать с учётными данными в соответствующем файле `config/*.toml`.

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
- Настройки обложек задаются в `[covers]` (`covers_path`, `cover_max_dimension_px`, `cover_jpeg_quality`, `show_covers`).
- В каталогах с книгами храните только поддерживаемые форматы, чтобы избежать сканирования лишних файлов.

## Reverse proxy

Для продакшен-настройки HTTPS через Nginx или Traefik см.:

- [`../service/proxy/README_RU.md`](../service/proxy/README_RU.md)
