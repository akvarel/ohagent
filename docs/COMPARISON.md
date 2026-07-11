# jcode vs ohAgent — Сравнительная характеристика

> **jcode** v0.37.0 (3cb1287e) — первичный инструмент с открытым исходным кодом под MIT.
> **ohAgent** v0.1.0 (d4cb42b) — OrangeHat AI Agent, 24/7 персональный ассистент на базе jcode.
>
> **Принцип:** ohAgent берёт runtime и движок jcode, надстраивая поверх инфраструктуру
> постоянного daemon-процесса, мульти-платформенные шлюзы и multi-tenant архитектуру.
>
> *Обновлять файл при каждой новой версии обоих агентов.*

---

## Инструкция по обновлению

При выходе новой версии **jcode** или **ohAgent**:

```bash
# 1. Обновить jcode submodule
cd /sharedssd/git/orangehat/ohAgent/jcode
git fetch origin master
git log --oneline master...HEAD  # посмотреть новые коммиты
# git checkout <new-tag-or-commit>

# 2. Обновить версии в этом файле
#    - jcode: заменить v0.37.0 (3cb1287e) на новую
#    - ohAgent: заменить v0.1.0 (d4cb42b) на новую

# 3. Проверить новые фичи jcode в README.md
less /sharedssd/git/orangehat/ohAgent/jcode/README.md

# 4. Обновить таблицы ниже — добавить/изменить строки
#    Проверить секции: Архитектура, Агентный движок, Инфраструктура

# 5. Проверить новые фичи ohAgent по git log
cd /sharedssd/git/orangehat/ohAgent
git log --oneline --all

# 6. Закоммитить изменения
git add docs/COMPARISON.md
git commit -m "docs: update comparison table — jcode v<new> vs ohAgent v<new>"
```

---

## 1. Архитектура

| Характеристика | jcode | ohAgent |
|---|---|---|
| **Тип** | CLI-инструмент (запуск по требованию) | Daemon (24/7 фоновый процесс) |
| **Запуск** | `jcode` → TUI сессия, `jcode run` → разовый вызов | `ohagent-daemon` → systemd-сервис |
| **Runtime** | Одно-сессионный, in-process | Многопоточный tokio, долгоживущий |
| **Провайдеры** | 40+ (Claude, OpenAI, Gemini, Copilot, Azure, OpenRouter, DeepSeek и др.) | Через jcode bridge + Vault + keys.toml |
| **Multi-tenant** | Нет — один пользователь | Да — tenant_id для каждого пользователя |
| **i18n** | Нет | Да — EN + LV + RU |
| **Session persistence** | Resume по имени, cross-harness (Claude Code, Codex, OpenCode, pi) | SQLite `sessions.db`, авто-восстановление при старте |
| **Graceful degradation** | Нет — если провайдер упал, сессия падает | Да — 4-фазная инициализация с health registry |

## 2. Интерфейсы

| Характеристика | jcode | ohAgent |
|---|---|---|
| **TUI** | Да — ratatui, собственный scrollback, 1000+ fps | Да — ohagent-cli (ratatui) |
| **Telegram gateway** | Нет | Да — бот с pairing-флоу |
| **WhatsApp gateway** | Нет | Да — Meta Cloud API webhook |
| **Slack gateway** | Нет | Да — Events API |
| **Web Dashboard** | Нет | Да — React SPA |
| **OpenAI-compatible REST API** | Нет | Да — `POST /v1/chat/completions`, `GET /v1/models` |
| **WebSocket streaming** | Нет (только внутренние каналы) | Да — `GET /v1/ws/chat` с tool call events |
| **Side panel** | Да — real-time diff viewer, mermaid | Нет |
| **Info widgets** | Да — занимают только негативное пространство | Нет |

## 3. Агентный движок

| Характеристика | jcode | ohAgent |
|---|---|---|
| **Provider routing** | Ручной выбор модели через `/model` | Автоматический роутер по capability |
| **Tool calling** | 30+ built-in tools (bash, write, edit, read, ls, grep, memory, browser, swarm и др.) | Те же через jcode bridge + tool registry |
| **Memory (semantic)** | Да — embedding graph с cosine similarity, sideagent верификация | Да — через ohagent-memory (SQLite + ONNX embeddings) |
| **Rolling summary** | Да | Да |
| **Session search (RAG)** | Да | Да |
| **Self-learning skills** | Trigger-based (embedding hit → auto-inject) | Да — Creator + Evaluator + Curator cron |
| **Swarm coordination** | Да — DAG, DM/broadcast, code-shifting detection | Да — через ohagent-swarm |
| **CMC Reasoning** | Нет | Да — Confidence Momentum Controller (30-70% токенов) |
| **Plugin pipeline** | Нет | Да — FFI chain: PII Redactor, Infra Launcher и др. |
| **MCP server pool** | Да — `~/.jcode/mcp.json` | Да — SharedMcpPool |
| **Браузерная автоматизация** | Да — Firefox Agent Bridge | Через jcode bridge |

## 4. Инфраструктура

| Характеристика | jcode | ohAgent |
|---|---|---|
| **Развёртывание** | CLI-бинарник, Homebrew, install script | Daemon + Docker + K8s (Kustomize) |
| **Secrets** | env vars, токены OAuth | Vault (HashiCorp) → env → `keys.toml` |
| **Rate limiter** | Нет | Да — sliding window per tenant |
| **Health checks** | Нет | Да — `/health` (Liveness + Readiness + Startup) |
| **Prometheus metrics** | Нет | Да — `/metrics` (requests, LLM calls, tokens, sessions) |
| **Message logging** | Нет | Да — SQLite + gzip + S3 Glacier archive |
| **Usage tracking** | Нет | Да — токены и стоимость per model/provider/tenant |
| **Scheduler / Cron** | Нет | Да — одноразовые напоминания + cron-задачи |
| **Push notifications** | Нет | Да — Telegram push через PushService |
| **Sandbox servers** | Нет | Да — Scaleway/Hetzner GPU instances |
| **Desktop MCP** | Нет | Да — ohagent-desktop-mcp (скриншот, мышь, клавиатура) |
| **CI/CD** | GitHub Actions + releases | GitLab CI (test → build kaniko → deploy) |

## 5. Безопасность

| Характеристика | jcode | ohAgent |
|---|---|---|
| **Multi-tenant isolation** | Нет | Да — query scope по tenant_id |
| **API authentication** | Нет (локальный CLI) | Да — X-API-Key / Bearer для `/api/*` |
| **Admin-only pairing** | Нет | Да — prevent self-pairing |
| **Plugin sandboxing** | Нет | Да — изолированные процессы |
| **Security guard** | Нет | Да — `check_command_safety()` |
| **Vault integration** | Нет | Да — resolution: Vault → env → keys.toml |

## 6. Производительность

| Характеристика | jcode | ohAgent |
|---|---|---|
| **RAM (1 session)** | ~27-167 MB | ~100-200 MB (с SQLite + embeddings) |
| **RAM per extra session** | ~10 MB | ~20-50 MB (с gateway) |
| **Startup time** | 14 ms to first frame | ~1-2 sec (SQLite init + Vault) |
| **TUI rendering** | 1000+ fps | ratatui (стандартная скорость) |

## 7. Self-Development

| Характеристика | jcode | ohAgent |
|---|---|---|
| **Self-dev mode** | Да — модифицирует свой код, пересобирается и перезагружается | Планируется |
| **Self-dev infrastructure** | build/test/reload toolset, debug socket | Нет |
| **Session resume from other harnesses** | Claude Code, Codex, OpenCode, pi | Нет |

## 8. Планы (Roadmap)

| Фича | jcode | ohAgent |
|---|---|---|
| **iOS app** | В разработке (Tailscale + OpenClaw) | Нет |
| **New git primitive** | В разработке | Нет |
| **Cargo build speed** | Цель: 5-20 sec incremental | — |
| **Human voice STT** | `jcode dictate` (внешняя команда) | Планируется |
| **Heroku-style dashboard** | — | Phase 5 (React SPA) |

---

## История изменений

### v0.1.0 — 2026-07-10
- Первый выпуск ohAgent
- Daemon + CLI TUI + WebSocket streaming
- Cancel mid-stream через `tokio::select!`
- Telegram + WhatsApp + Slack gateways
- Vault integration + graceful degradation
- Multi-tenant, i18n (EN+LV+RU)
- Swarm orchestration + CMC reasoning
- Self-learning skills cron
- Session persistence + message logging + usage tracking
- K8s deployment manifests + GitLab CI

### jcode v0.37.0 — актуальная версия
- 40+ provider integrations (OAuth + API key)
- Swarm file-shift detection
- Memory graph with semantic embeddings
- Side panel + mermaid inline rendering
- 1000+ fps TUI rendering
- Self-dev mode
- Browser automation (Firefox Agent Bridge)
- Cross-harness session resume
- Ambient mode
