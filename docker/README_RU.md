# Развёртывание ROPDS в Docker

Готовые сценарии Docker Compose для запуска ROPDS:

- SQLite (рекомендуемый вариант по умолчанию)
- PostgreSQL 16+ (в комплекте или внешний)
- MariaDB 11+ (в комплекте или внешний)
- MySQL 8+ (внешний)

## Что понадобится

- Docker Engine + Docker Compose v2
- Директория на хост-машине для библиотеки (книги, обложки, загрузки)

## Быстрый старт без клонирования репозитория

Чтобы развернуть последний релиз ROPDS на своём сервере без клонирования репозитория:

1. Выберите тип базы данных: `sqlite`, `postgres.sibling`, `postgres.external`, `mysql.sibling` или `mysql.external`.

2. Со страницы [последнего релиза](https://github.com/dshein-alt/ropds/releases/latest) скачайте три файла в пустой каталог (подставьте выбранный тип вместо `<flavor>`):

```bash
FLAVOR=sqlite   # либо postgres.sibling / postgres.external / mysql.sibling / mysql.external
BASE=https://github.com/dshein-alt/ropds/releases/latest/download

mkdir ropds && cd ropds
curl -LO "$BASE/docker-compose.$FLAVOR.yml"
curl -Lo config.toml "$BASE/config.$FLAVOR.toml"
curl -Lo .env        "$BASE/.env.example"
```

3. Отредактируйте `.env` (как минимум задайте `ROPDS_ADMIN_PASSWORD`) и `config.toml` (секрет сессии, базовый URL, учётные данные БД для postgres/mysql).

4. Запустите сервис:

```bash
docker compose -f docker-compose.$FLAVOR.yml up -d
```

5. Откройте `http://localhost:8081/web` для веб-интерфейса или `http://localhost:8081/opds` для OPDS-ленты.

**Чтобы использовать Docker Hub вместо GHCR:** перед `up` задайте в `.env` значение `ROPDS_IMAGE=docker.io/dsheinalt/ropds`.

**Чтобы зафиксировать конкретную версию:** перед `up` задайте в `.env` значение `ROPDS_VERSION=0.10.5`.

## Быстрый старт из исходников (для разработчиков)

Для тех, кто хочет собрать ROPDS из локального дерева исходников вместо загрузки опубликованного образа, используйте файл переопределения `docker-compose.build.override.yml`:

1. Скопируйте файл окружения и задайте `ROPDS_CONFIG_FILE` для выбранного варианта БД:

```bash
cp docker/.env.example docker/.env
# Отредактируйте docker/.env: задайте ROPDS_CONFIG_FILE=./config/config.sqlite.toml
```

2. Соберите и запустите из директории `docker/`:

```bash
cd docker
docker compose \
  -f docker-compose.sqlite.yml \
  -f docker-compose.build.override.yml \
  up -d --build
```

Параметр `image: ropds:local` из файла переопределения имеет приоритет при слиянии, поэтому значения `ROPDS_IMAGE` и `ROPDS_VERSION` из `.env` в этом режиме игнорируются.

3. Откройте `http://localhost:8081/web` или `http://localhost:8081/opds`.

## Сценарии compose (для разработчиков)

Каждый вариант — отдельный самодостаточный compose-файл. После указания `ROPDS_CONFIG_FILE` в `docker/.env` запустите из директории `docker/` с файлом переопределения сборки:

| Вариант | Команда |
|---|---|
| SQLite (БД на томе) | `docker compose -f docker-compose.sqlite.yml -f docker-compose.build.override.yml up -d --build` |
| PostgreSQL (в комплекте) | `docker compose -f docker-compose.postgres.sibling.yml -f docker-compose.build.override.yml up -d --build` |
| PostgreSQL (внешний сервер) | `docker compose -f docker-compose.postgres.external.yml -f docker-compose.build.override.yml up -d --build` |
| MySQL/MariaDB (в комплекте) | `docker compose -f docker-compose.mysql.sibling.yml -f docker-compose.build.override.yml up -d --build` |
| MySQL/MariaDB (внешний сервер) | `docker compose -f docker-compose.mysql.external.yml -f docker-compose.build.override.yml up -d --build` |

Варианты **«в комплекте»** включают и ROPDS, и базу данных в одном compose-файле — всё запускается одной командой.

**«Внешний сервер»** — запускается только ROPDS, база данных размещена отдельно.

## Конфигурационные файлы

Конфиги приложения лежат в `docker/config/`:

- `config.sqlite.toml` — готовая конфигурация для Docker (SQLite), используется в варианте с SQLite
- `config.postgres.sibling.toml`
- `config.postgres.external.toml`
- `config.mysql.sibling.toml`
- `config.mysql.external.toml`

В выбранном файле проверьте и настройте:

- `server.base_url`: внешний адрес сервера
- `[database].url`: для вариантов с внешней БД

Для локальной проверки достаточно `server.base_url = "http://localhost:8081"`.

## Монтирование и структура данных

Каждый compose-файл монтирует:

- `${ROPDS_CONFIG_FILE:-./config.toml} -> /app/config/config.toml` (только для чтения) — по умолчанию `./config.toml` для автономного развёртывания, или укажите `./config/config.<flavor>.toml` при работе из исходников
- `${ROPDS_LIBRARY_ROOT} -> /library` (чтение и запись) — по умолчанию `./library`

Веб-шаблоны, статика и файлы локализации встроены в исполняемый файл при сборке.

При работе приложение создаёт и использует:

- `/library/covers`
- `/library/uploads`
- Том SQLite по пути `/var/lib/ropds/sqlite` (только в варианте с SQLite)

## Переменные окружения

Полный список — в `docker/.env.example`. Основные:

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `ROPDS_IMAGE` | `ghcr.io/dshein-alt/ropds` | Репозиторий образа (без тега) |
| `ROPDS_VERSION` | `latest` | Тег образа для загрузки |
| `ROPDS_CONFIG_FILE` | `./config.toml` | Путь к файлу конфигурации на хосте относительно compose-файла |
| `TZ` | (нет) | Часовой пояс контейнера (напр. `Europe/Moscow`) |
| `ROPDS_PORT` | `8081` | HTTP-порт на хосте |
| `ROPDS_LIBRARY_ROOT` | `./library` | Путь к библиотеке на хосте |
| `ROPDS_ADMIN_PASSWORD` | (нет) | Пароль администратора при первом запуске |
| `ROPDS_ADMIN_INIT_ONCE` | `true` | Однократная инициализация администратора |
| `ROPDS_DB_WAIT_TIMEOUT` | `60` | Тайм-аут ожидания базы данных (секунды) |
| `ROPDS_DB_HOST` | (нет) | Явное указание хоста БД |
| `ROPDS_DB_PORT` | (нет) | Явное указание порта БД |

Для вариантов со встроенной базой данных (PostgreSQL и MySQL/MariaDB):

| Переменная | По умолчанию | Назначение |
|---|---|---|
| `DB_NAME` | `ropds` | Имя базы данных |
| `DB_USER` | `ropds` | Пользователь БД |
| `DB_PASSWORD` | `ropds_change_me` | Пароль пользователя БД |
| `DB_ROOT_PASSWORD` | `root_change_me` | Root-пароль MariaDB (только для MySQL) |

Значения должны совпадать с учётными данными в соответствующем `config/*.toml`.

## Поведение при старте

**Создание администратора.** Entrypoint умеет запускать `ropds --set-admin` автоматически — задайте `ROPDS_ADMIN_PASSWORD` в `docker/.env`. При `ROPDS_ADMIN_INIT_ONCE=true` (по умолчанию) это происходит один раз, после чего создаётся маркер `/library/.ropds_admin_initialized`. Установите `false`, чтобы пароль обновлялся при каждом запуске.

**Ожидание БД.** Если используется PostgreSQL или MySQL, entrypoint дожидается доступности порта БД, прежде чем запускать приложение. Адрес ожидания можно переопределить через `ROPDS_DB_HOST` и `ROPDS_DB_PORT`.

**Миграции.** Применяются автоматически при старте в зависимости от выбранного бэкенда.

## Безопасность

- Перед выводом в продакшен обязательно смените `ROPDS_ADMIN_PASSWORD` и `session_secret`.
- Для `ROPDS_LIBRARY_ROOT` используйте абсолютный путь.
- Конфигурационный файл монтируйте в режиме только для чтения.
- Не меняйте `session_secret` между перезапусками — иначе сессии пользователей сбросятся.
- `ROPDS_ADMIN_PASSWORD` передаётся как аргумент командной строки и может быть виден в списке процессов (`/proc/PID/cmdline`, `docker inspect`). В чувствительных окружениях лучше выполнить `ropds --set-admin` вручную внутри контейнера.

## Организация библиотеки

- По умолчанию `covers_path` и `upload_path` указывают внутрь `/library`.
- Параметры обложек задаются в `[covers]` (`covers_path`, `cover_max_dimension_px`, `cover_jpeg_quality`, `show_covers`).
- Храните в каталогах с книгами только файлы поддерживаемых форматов — так сканер не будет тратить время на лишнее.

## Reverse proxy

Для продакшена с HTTPS через Nginx или Traefik см. [`../service/proxy/README_RU.md`](../service/proxy/README_RU.md).
