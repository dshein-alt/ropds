# Reverse proxy (Nginx / Traefik)

ROPDS работает за reverse proxy без дополнительной настройки со стороны приложения.

Установка как PWA (service worker + manifest) поддерживается, только если сайт доступен по:
- `https://...`
- `http://localhost` (только для разработки)

## Nginx (HTTPS)

```nginx
server {
    listen 80;
    listen [::]:80;
    server_name books.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name books.example.com;

    ssl_certificate     /etc/letsencrypt/live/books.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/books.example.com/privkey.pem;

    client_max_body_size 100m;

    location / {
        proxy_pass http://127.0.0.1:8081;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 300s;
    }
}
```

## Traefik (Docker labels)

Для случая, когда ROPDS запущен в Docker/Podman, а TLS терминируется на Traefik.

```yaml
services:
  ropds:
    image: ghcr.io/dshein-alt/ropds:latest
    container_name: ropds
    labels:
      - traefik.enable=true
      - traefik.http.routers.ropds.rule=Host(`books.example.com`)
      - traefik.http.routers.ropds.entrypoints=websecure
      - traefik.http.routers.ropds.tls.certresolver=letsencrypt
      - traefik.http.services.ropds.loadbalancer.server.port=8081
```

## Traefik (dynamic file provider)

Для случая, когда ROPDS работает прямо на хосте, а Traefik настраивается через динамический конфиг.

```yaml
http:
  routers:
    ropds:
      rule: Host(`books.example.com`)
      entryPoints:
        - websecure
      service: ropds
      tls:
        certResolver: letsencrypt

  services:
    ropds:
      loadBalancer:
        servers:
          - url: "http://127.0.0.1:8081"
```

## На заметку

- Не меняйте `session_secret` в `config.toml` между перезапусками — иначе сбросятся все сессии.
- Если включена загрузка книг, задайте лимит размера запроса (например `client_max_body_size`) не ниже значения `upload.max_upload_size_mb`.
- Пробрасывайте заголовки `Host` и `X-Forwarded-*`, чтобы ссылки и логи отражали внешний адрес сервера.
