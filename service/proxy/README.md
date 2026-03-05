# Reverse proxy (Nginx / Traefik)

ROPDS works behind a reverse proxy without any special application-side configuration.

PWA installation (service worker + manifest) is supported only when the site is served from:
- `https://...`
- `http://localhost` (development)

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

For ROPDS running in Docker/Podman with Traefik handling TLS.

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

For ROPDS running directly on the host, when Traefik is configured via a dynamic config file.

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

## Practical notes

- Keep `session_secret` in `config.toml` stable across restarts, otherwise all user sessions are invalidated.
- If uploads are enabled, set `client_max_body_size` (or equivalent) to match `upload.max_upload_size_mb`.
- Pass `Host` and `X-Forwarded-*` headers through so that generated links and logs reflect the client-facing URL.
