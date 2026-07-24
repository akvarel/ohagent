# Infrastructure — Local Services

## PostgreSQL + pgvector (Local Docker)

| Поле | Значение |
|---|---|
| Контейнер | `orangehat-memory-db` |
| Образ | `pgvector/pgvector:pg16` |
| Порт | `5432` |
| DB | `orangehat_memory` |
| User | `ohmemory` |
| Restart | `unless-stopped` (автостарт после reboot) |
| Volume | `/var/lib/docker/volumes/.../_data` (persistent) |
| Записей | ~5.7K, 48 MB |

### Подключение
```
postgresql://ohmemory:ohmemory@localhost:5432/orangehat_memory
```

### Автозапуск после перезагрузки
Система включает Docker на boot, Docker запускает контейнер по `unless-stopped`:
1. `systemctl enable docker` ✅
2. `docker update --restart unless-stopped orangehat-memory-db` ✅
