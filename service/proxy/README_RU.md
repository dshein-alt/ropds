# Примеры reverse proxy

ROPDS можно запускать за reverse proxy без изменений в приложении.

Установка как PWA (service worker + manifest) в браузерах работает только на:
- `https://...`
- `http://localhost` (локальная разработка)

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

Вариант для запуска ROPDS в Docker/Podman, когда TLS завершается в Traefik.

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

Вариант для запуска ROPDS прямо на хосте (без labels).

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

## Примечания

- `session_secret` в `config.toml` должен быть постоянным между перезапусками.
- Если включена загрузка книг, задайте лимит body/request у прокси не ниже `upload.max_upload_size_mb`.
- Передавайте `Host` и `X-Forwarded-*`, чтобы логи и ссылки соответствовали внешнему URL/схеме.
