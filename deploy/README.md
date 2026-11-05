# Деплой PoP - Protocol на VPS

## Требования
- VPS с Docker (Ubuntu 22.04+: `curl -fsSL https://get.docker.com | sh`)
- 1 CPU / 1 ГБ RAM минимум, порт 80/443 свободен

## Запуск (docker compose)
```bash
git clone <repo> pop-protocol && cd pop-protocol
docker compose up -d --build
curl -s http://localhost:8080/health   # ok
```
Сервер поднимется на 8080, MongoDB — внутри compose, данные в volume `mongo-data`.

## HTTPS / wss (нужно для игры из браузера)
Игра использует WebSocket `wss://…/ws` — обязателен TLS. Вариант — Caddy (автоматический Let's Encrypt):
```
# /etc/caddy/Caddyfile
game.example.com {
    reverse_proxy localhost:8080
}
```
`systemctl reload caddy` — и клиенты играют на `https://game.example.com`.

## Обновление
```bash
git pull
docker compose up -d --build
```

## Переменные
- `PORT` — порт HTTP/WS (8080 в контейнере)
- `MONGO_URL` — строка подключения (compose указывает `mongodb://mongo:27017`)
- `RUST_LOG` — уровень логов (`info`, `debug`)

## Отладочные переменные (не для продакшена)
`EVENT_TIMER_SECS` — интервал евентов; `POPGO_START_LEVEL` — стартовый уровень (3 = босс).

## Мониторинг
- `GET /health` → `ok`
- `GET /stats` → `{ws_clients, version, …}`
- `GET /api/rooms`, `GET /api/leaderboard`
