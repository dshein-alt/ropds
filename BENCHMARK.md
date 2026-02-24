# ROPDS Performance Benchmark

## Setup

- **Binary:** Release build (`cargo build --release`)
- **Database:** SQLite (fresh instance)
- **Log level:** warn
- **Tool:** Apache Bench (`ab2`)
- **Host:** localhost
- **OS/Kernel:** ALT Workstation K 11.2 / Linux 6.12.65
- **CPU:** AMD Ryzen 7 8745HS w/ Radeon 780M Graphics
- **Date:** 2026-02-24

## Results

| Endpoint | Requests | Concurrency | Keep-Alive | Req/sec | Avg Latency | p99 | p100 |
|---|---|---|---|---|---|---|---|
| `GET /health` (JSON) | 10,000 | 50 | No | 29,399 | 1.7ms | 2ms | 10ms |
| `GET /health` (high concurrency) | 50,000 | 200 | No | 29,523 | 6.8ms | 8ms | 10ms |
| `GET /web/login` (HTML template) | 10,000 | 50 | No | 29,226 | 1.7ms | 2ms | 3ms |
| `GET /web/login` (keep-alive) | 10,000 | 100 | Yes | 135,225 | 0.7ms | 2ms | 4ms |
| `GET /static/css/bootstrap.min.css` (233KB) | 10,000 | 50 | No | 11,252 | 4.4ms | 6ms | 10ms |
| `GET /opds` (Basic Auth + Argon2) | 10,000 | 50 | No | 295 | 170ms | 310ms | 338ms |

## Observations

- **~29.5K req/s** for lightweight endpoints without keep-alive, scaling to **135K req/s** with keep-alive enabled.
- Increasing concurrency from 50 to 200 on `/health` maintained the same throughput with zero failed requests and tight latency (p100 = 10ms).
- Static file serving pushes ~2.5 GB/s transfer rate at 11.2K req/s for a 233KB file.
- OPDS with Basic Auth + Argon2 password verification is ~295 req/s. This is expected — Argon2 is intentionally slow for security, and each OPDS request re-verifies the password (stateless Basic Auth).
