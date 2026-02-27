# Reverse Proxy Examples

ROPDS runs behind a reverse proxy without extra app changes.

PWA install support (service worker + manifest) works in browsers only on:
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

Use when ROPDS is started in Docker/Podman and Traefik handles TLS.

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

Use when ROPDS runs directly on host (not in Docker labels flow).

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

## Notes

- Keep `session_secret` stable in `config.toml` across restarts.
- If uploads are enabled, set a proxy/body size limit that matches your `upload.max_upload_size_mb`.
- Preserve `Host` and `X-Forwarded-*` headers so generated links and logs reflect client-facing URL/scheme.
