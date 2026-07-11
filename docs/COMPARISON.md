# jcode vs ohAgent — Сравнительная характеристика

> **jcode** v0.43.0 (c4b2efe4) — первичный инструмент с открытым исходным кодом под MIT.
> **ohAgent** v0.1.0 (bfba272) — OrangeHat AI Agent, 24/7 персональный ассистент на базе jcode.
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
#    - jcode: заменить v0.43.0 (c4b2efe4) на новую
#    - ohAgent: заменить v0.1.0 (bfba272) на новую

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
| **Mermaid rendering** | Да — inline, собственный рендерер (1800x быстрее) | Нет |
| **LaTeX rendering** | Да — терминальная математика | Нет |
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

### ohAgent v0.1.0 — 2026-07-10
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

### jcode v0.43.0 (c4b2efe4) — 2026-07-11 (обновление с v0.37.0)
- LaTeX rendering в терминале
- Swarm agent cards под spawn calls
- Поддержка common LaTeX containers
- TUI fixes, упрощение low-confidence guidance

---

# ohAgent vs Hermes Agent — Сравнительная характеристика

> **ohAgent** v0.1.0 — Rust-based daemon (OrangeHat), построен на Jcode engine.
> **Hermes Agent** v0.18.2 (2026-07-07) — Python-based self-improving agent от Nous Research.
>
> Оба — open-source (MIT), оба — автономные AI-агенты.
> ohAgent наследует производительность Rust/Jcode. Hermes наследует Python-экосистему.

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **Язык** | Rust (через jcode) | Python 82% + TypeScript 15% |
| **Лицензия** | MIT (через jcode) | MIT |
| **GitHub звёзды** | — | 213k |
| **Архитектура** | Daemon (24/7 tokio), мульти-тред | CLI + Gateway, Python asyncio |
| **Установка** | `cargo build --release` | `curl ... install.sh \| bash` |
| **Размер бинарника** | ~50 MB (Rust release) | ~100+ MB (Python + venv) |
| **Платформы** | Linux + macOS | Linux, macOS, Windows (native), WSL2, Termux |

## 1. Архитектура и развёртывание

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **Тип процесса** | 24/7 daemon (systemd) | CLI + Gateway (asyncio) |
| **Backend-ы** | Локальный, Docker, K8s | Local, Docker, SSH, Singularity, Modal, Daytona |
| **Serverless** | Нет | Да (Modal, Daytona — hibernate на idle) |
| **Desktop app** | Нет | Да — macOS, Windows, Linux (native) |
| **VPS запуск** | systemd + cargo run | `hermes gateway` + VPS |
| **Graceful degradation** | Да — 4 фазы инициализации, health registry | Нет — частичная |
| **Multi-tenant** | Да — tenant_id для каждого пользователя | Нет — один пользователь |
| **i18n** | Да — EN + LV + RU | Нет (только EN, но есть локали в репозитории) |
| **Secrets** | Vault → env → keys.toml | `.env` файл, env vars |

## 2. Интерфейсы и платформы

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **TUI** | ohagent-cli (ratatui) | Да — свой TUI с autocomplete, interrupt, streaming |
| **Telegram** | Да — бот с pairing | Да |
| **Discord** | Нет (планируется) | Да |
| **Slack** | Да — Events API | Да |
| **WhatsApp** | Да — Meta Cloud API | Да |
| **Signal** | Нет | Да |
| **Email** | Нет (планируется) | Да |
| **Home Assistant** | Нет | Да |
| **Web Dashboard** | React SPA (планируется) | Нет |
| **Voice/TTS** | Нет | Да — голосовые заметки, TTS |
| **OpenAI-compatible API** | Да — `POST /v1/chat/completions` | Нет |
| **WebSocket streaming** | Да — `GET /v1/ws/chat` | Нет |

## 3. Агентный движок

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **Self-learning loop** | Да — Creator/Evaluator/Curator cron | Да — closed learning loop (key USP) |
| **Авто-создание навыков** | Да — из повторяющихся паттернов | Да — после сложных задач |
| **Улучшение навыков** | Оценка качества (Q-score), promotion | Self-improve во время использования |
| **Skills marketplace** | Нет | Да — Skills Hub (90k+ community skills) |
| **Memory (persistent)** | SQLite + rolling summary + semantic search | FTS5 + LLM summarization + Honcho моделирование |
| **User modeling** | Нет | Да — Honcho dialectic user modeling |
| **Provider routing** | Автоматический роутер по capability | `/model` ручной выбор |
| **Модели** | Через jcode (40+ провайдеров) | 300+ через Nous Portal + OpenRouter + любые |
| **Sub-agents** | Да — swarm через ohagent-swarm (DAG) | Да — spawn изолированных subagents (RPC) |
| **CMC Reasoning** | Да — Confidence Momentum Controller | Нет |
| **Cron scheduler** | Да — push-уведомления + cron | Да — встроенный cron с доставкой на платформы |
| **Personality system** | Нет | Да — `/personality [name]` |
| **Context files** | AGENTS.md через system_prompt_builder | Да — AGENTS.md, context files |
| **Tools count** | 30+ через jcode bridge | 40+ tools, toolset system |
| **MCP** | Да — SharedMcpPool | Да |

## 4. Инфраструктура

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **Health checks** | Да — `/health` | Нет |
| **Prometheus metrics** | Да — `/metrics` | Нет |
| **Rate limiter** | Да — sliding window per tenant | Нет |
| **Message logging** | Да — SQLite + gzip + S3 Glacier | Нет |
| **Usage tracking** | Да — токены/стоимость per model/tenant | Нет |
| **Push notifications** | Да — Telegram push | Нет (только ответы в чатах) |
| **Vault integration** | Да — HashiCorp Vault | Нет — `.env` file |
| **CI/CD** | GitLab CI (test → kaniko → deploy) | GitHub Actions |
| **K8s deployment** | Да — Kustomize manifests | Docker compose |
| **Sandbox servers** | Да — Scaleway/Hetzner GPU | Нет |
| **Desktop MCP** | Да — ohagent-desktop-mcp | Нет (но есть community решения) |
| **Batch training** | Нет | Да — batch trajectory generation |

## 5. Безопасность

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **Multi-tenant isolation** | Да | Нет |
| **API authentication** | Да — X-API-Key / Bearer | Нет (локальный) |
| **Admin-only pairing** | Да | Нет |
| **Plugin sandboxing** | Да — изолированные FFI | Нет |
| **Security guard** | Да — проверка команд | Да — command approval |
| **Secrets management** | Vault (HashiCorp) → env → keys.toml | `.env` file |

## 6. Производительность

| Характеристика | ohAgent | Hermes Agent |
|---|---|---|
| **RAM (1 сессия)** | ~100-200 MB | ~150-400 MB (Python + Node.js) |
| **Startup** | ~1-2 sec | ~5-30 sec (Python venv + Node.js) |
| **Язык** | Rust (компилируемый) | Python (интерпретируемый) |

## 7. Ключевые отличия

**ohAgent сильнее где:**
- Безопасность — Vault, multi-tenant, audit trails
- Инфраструктура — K8s, health checks, Prometheus, rate limiter
- Rust-производительность
- Telegram + Slack + WhatsApp из коробки
- CMC Reasoning (экономия токенов)

**Hermes сильнее где:**
- Self-learning loop — глубже, автономнее
- Skills Hub — 90k community skills
- Больше платформ (Signal, Discord, Email, Home Assistant)
- Desktop app (macOS/Windows/Linux)
- Serverless (Modal/Daytona)
- Nous Portal — 300+ моделей под одной подпиской
- Batch trajectory generation (ML research)
- Voice/TTS

---

# ohAgent vs Альтернативы Hermes (из обзора Composio)

Сводная таблица позиционирования ohAgent среди 11 альтернатив Hermes из
обзора [Composio](https://composio.dev/content/hermes-agent-alternatives) (May 2026).

## Open Source альтернативы

| Инструмент | Язык | Установка | Self-hosted | Уникальная фича | ohAgent vs |
|---|---|---|---|---|---|
| **Hermes Agent** | Python | 5 min | Да | Self-learning loop, Skills Hub | ohAgent проигрывает в эко-системе навыков и кол-ве платформ, выигрывает в безопасности и инфраструктуре |
| **OpenClaw** | Node.js | 30-60 min | Да | 24 платформы, 52 built-in навыка, 13k ClawHub | ohAgent: Rust вместо Node.js, Vault, health checks, K8s. OpenClaw: 24+ платформы, ClawHub (но 20% malicious пакетов) |
| **TrustClaw** | OSS + Cloud | 1 min | Да/Облако | OAuth-only, sandboxed execution, 20k Composio tools | Разные цели — TrustClaw про безопасность облачных интеграций, ohAgent про self-hosted daemon |
| **PicoClaw** | Go | 2 min | Да | 10 MB binary, $10 RISC-V hardware | ohAgent тяжелее, но функциональнее. PicoClaw — embedded |
| **ZeroClaw** | Rust | 5 min | Да | 3.4 MB, sub-10ms startup | ohAgent использует тот же Rust/jcode, но тяжелее за счёт фич |
| **nanobot** | Python | 2 min | Да | 4000 строк, MCP support | ohAgent: Rust performance, Vault, K8s. nanobot: auditable Python |
| **memU Bot** | Python | 10 min | Да | File-system memory, proactivity engine | ohAgent: SQLite memory + rolling summary. memU: глубже structured memory |

## Managed (хостинг) альтернативы

| Инструмент | Цена | Уникальная фича | ohAgent vs |
|---|---|---|---|
| **Perplexity Computer** | $200/mo Max | 19-model orchestration, parallel sub-agents | ohAgent: self-hosted, бесплатно + API costs. Perplexity: мультимодельное исследование |
| **Claude Cowork** | $20/mo Pro | Desktop agent, macOS Accessibility API | ohAgent: 24/7 daemon, не desktop. Cowork: ваши личные приложения |
| **KimiClaw** | Подписка Kimi | 40GB storage, RAG, K2.5 model, BYO Claw | ohAgent: self-hosted, любые модели. KimiClaw: привязан к Moonshot AI |
| **Manus** | Free / $20 Pro | Full virtual computer, Meta-acquired | ohAgent: daemon. Manus: task executor |
| **Vellum** | Free download | Device-first, credential isolation, proactivity | ohAgent: server daemon. Vellum: local device AI |

## Позиционирование ohAgent

```
                  Безопасность / Enterprise
                           │
                 ohAgent   │   TrustClaw
                           │
      ─────────────────────┼─────────────────────
                           │
     Hermes ───────────────┤
     OpenClaw              │
     nanobot, memU         │
     PicoClaw, ZeroClaw    │
                           │
                Self-hosted / Гиковский
```

**ohAgent занимает нишу Rust-powered self-hosted daemon с enterprise-grade инфраструктурой:**

1. **Единственный на Rust** среди всех альтернатив (кроме ZeroClaw, но тот минималистичен)
2. **Vault first** — никаких `.env` файлов с секретами (Hermes, OpenClaw, все остальные — `.env`)
3. **Enterprise-ready** — multi-tenant, health checks, metrics, rate limiter, audit log
4. **Jcode engine** — наследует 30+ built-in tools, browser automation, swarm
5. **K8s native** — Kustomize manifests для production deployment

**Главные пробелы относительно Hermes и OpenClaw:**
- Меньше платформ (нет Discord, Signal, Email, Home Assistant)
- Нет desktop app
- Нет serverless (Modal/Daytona)
- Меньше community skills
- Нет voice/TTS
