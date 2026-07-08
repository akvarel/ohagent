# Product Lens Review — ohAgent

> Date: 2026-07-08
> Review type: Full diagnostic (Mode 1 + Mode 2 + Mode 4)
> Status: HONEST — no sugar-coating

---

## Mode 1: Product Diagnostic

### 1. Who is this for?

**Original vision:** Sergey — a single power user who wants a 24/7 AI in Telegram.

**Current direction:** Multi-tenant platform for 10K users with aggregator billing, K8s HPA, sandbox pods, PostgreSQL + DragonflyDB.

**The gap:** These are two different products. Building enterprise infrastructure for a single-user tool is premature optimization. There is NO second user. There is NO billing. Yet we have 15 crates, aggregator DB schema, and sandbox architecture docs.

**Recommendation:** Re-center on Sergey as the ONLY user. Remove multi-tenant abstractions that add complexity without value. Multi-tenant can be layered later — when there IS a second tenant.

### 2. What's the pain?

| Pain | Quantified | Solved? |
|---|---|---|
| "I want AI in Telegram, always on" | Daily use | ✅ Daemon + Telegram gateway |
| "I want it to remember context" | Every conversation | ✅ Memory engine |
| "I want receipt OCR" | Weekly (15-20 receipts) | ✅ Gemini pipeline, 4s, free |
| "I want it to learn my patterns" | Ongoing | ⚠️ Skills engine works but no real usage data |
| "I want routing to cheapest model" | Every request | ✅ Provider metrics + routing |
| "I want people recognition in photos" | As needed | ✅ Tested, 16 models benchmarked |

**What's NOT solved for Sergey personally:**
- Can't use it right now (no running instance — daemon not deployed)
- No dashboard to see memory/skills/usage
- No WhatsApp (his primary messenger?)

### 3. Why now?

- Jcode v0.37.0 is stable, headless API works
- Gemini OCR is free and 4x faster than alternatives
- Vision model sweep complete — we KNOW which models work
- K8s cluster is available (orangehat namespace)

**The window:** 1-2 weeks to ship a polished single-user version. After that, attention may shift.

### 4. What's the 10-star version?

- ohAgent running 24/7 in my K8s
- Answers in Telegram AND WhatsApp
- Remembers everything — auto-summarizes, proactive nudges
- OCRs any receipt I send, categorizes, exports to accounting
- Recognizes people in photos, tags them
- Self-learns: "Sergey always asks about K8s deployments → here's a skill for that"
- Dashboard shows: memory graph, skill quality, provider costs, model performance
- Other people can sign up, pair, and get their own isolated agent

### 5. What's the MVP?

**One user (Sergey), one platform (Telegram), three core features:**

1. **Chat** — Jcode agent in Telegram, always on
2. **Memory** — persistent across sessions, searchable
3. **OCR** — /ocr command for receipts

That's it. No aggregator. No sandbox. No WhatsApp. No desktop MCP. No billing.

**The MVP is 90% built.** What's missing:
- Deploy it to K8s (daemon is ready, just not running)
- Actually use it for a week
- Fix what breaks

### 6. Anti-goals (explicitly NOT building NOW)

- Multi-user billing and aggregator
- Per-tenant sandbox pods
- WhatsApp/Slack gateways
- Desktop MCP plugin
- Plugin SDK for third-party developers
- OpenAI-compatible API endpoint for external consumers
- Revenue optimization / markup tiers

These are documented in `docs/` for when they're needed. They are NOT in the MVP.

### 7. How do we know it's working?

| Metric | Target | How to measure |
|---|---|---|
| Messages processed | >50/day | Prometheus counter |
| Memory recall hit rate | >30% | Track recall vs. new responses |
| OCR accuracy | >95% | Arbiter pass rate |
| Uptime | 99% | Prometheus uptime probe |
| Cost | <€5/mo | Provider usage tracking |
| User satisfaction | "It works" | Qualitative — just use it |

---

## Mode 2: Founder Review

### What is this trying to be?

Reading the code, commits, and docs, ohAgent is trying to be **three things at once:**

1. **Personal AI assistant** (daemon, Telegram, memory, skills, OCR)
2. **AI platform** (multi-tenant, aggregator, billing, sandbox, plugins)
3. **Infrastructure product** (K8s operator, Prometheus, HPA, Vault)

### Scoring

| Signal | Score | Evidence |
|---|---|---|
| **Product-market fit** | 2/10 | Zero users outside Sergey. Platform features built for imaginary users. |
| **Usage growth** | 0/10 | Not deployed. No metrics. No users. |
| **Retention** | N/A | Nothing to retain. |
| **Revenue** | 0/10 | No billing. Aggregator is architecture, not code. |
| **Competitive moat** | 5/10 | Jcode integration is unique. Vision model benchmarks are unique. Multi-provider routing is real. |
| **Code quality** | 7/10 | Rust, tested, idiomatic. But 15 crates for single-user tool is over-engineering. |

### The one thing that would 10x this

**Deploy it and use it.** The project has 24,500 lines of Rust, 15 crates, K8s configs, Prometheus alerts, aggregator schemas — and ZERO runtime hours. Ship the daemon to K8s. Use it for a week. Everything else is architecture fiction.

### Things we're building that don't matter (right now)

1. **Aggregator billing** (2 crates, 372 lines) — for a product with no users and no billing
2. **Sandbox architecture** (`docs/SANDBOX.md`, 200+ lines) — premature without user demand
3. **Plugin SDK** (`docs/PLUGINS.md`) — SDK before anyone asked for one
4. **Desktop MCP** (1,162 lines) — Sergey doesn't use desktop Jcode
5. **PII redactor** (210 lines) — no legal requirement, no users
6. **Infra launcher** (510 lines) — duct tape for what K8s already does

**Total waste:** ~2,500 lines of code + ~500 lines of docs that serve no current user.

---

## Mode 4: Feature Prioritization (ICE)

| Feature | Impact | Confidence | Effort | ICE | Priority |
|---|---|---|---|---|---|
| **Deploy to K8s (actually run it)** | 5 | 5 | 1 | **25** | 🔴 NOW |
| **Use it for a week, fix bugs** | 5 | 4 | 3 | **6.7** | 🔴 NOW |
| **Dashboard (React, actually show data)** | 3 | 4 | 4 | **3.0** | 🟡 NEXT |
| **Vision consensus in production** | 2 | 4 | 2 | **4.0** | 🟡 NEXT |
| **WhatsApp gateway** | 3 | 3 | 5 | **1.8** | 🟢 LATER |
| **Aggregator billing** | 1 | 2 | 5 | **0.4** | ⚪ KILL |
| **Sandbox pods** | 1 | 2 | 4 | **0.5** | ⚪ KILL |
| **Plugin SDK** | 1 | 1 | 5 | **0.2** | ⚪ KILL |

---

## Honest Assessment

### What's GOOD

- **Jcode bridge is solid.** Headless sessions, streaming, swarm integration — this is the hard part and it works.
- **Memory engine is real.** SQLite + vector, semantic search, nudges, summaries. 1,816 lines, tested.
- **Skills engine is real.** Creator, evaluator, curator. Lifecycle management. 1,430 lines.
- **Receipt OCR is solved.** Gemini → Arbiter, 4s, free, 4/4 accuracy.
- **People recognition is solved.** 16 models benchmarked, mistral-small wins at $0.0004/photo.
- **Provider routing is real.** 8 providers, 43 models, price-aware selection.
- **K8s deployment is written.** ConfigMaps, HPA, Prometheus, zero-downtime. Just not applied.
- **Documentation is excellent.** 9 docs, clear, honest. MODEL-GUIDE with anti-patterns is rare quality.

### What's BAD

- **Not deployed.** 50+ commits, 0 runtime hours. This is the cardinal sin of building.
- **Scope creep.** From "personal AI assistant" to "multi-tenant AI platform with aggregator billing." We built infrastructure for problems we don't have.
- **15 crates for one user.** `ohagent-aggregator-core`, `ohagent-infra-launcher`, `ohagent-pii-redactor` — these solve problems for a company with 1000 users. We have 1.
- **README claims things that aren't built.** WhatsApp/Slack in architecture diagram. Dashboard is 0 lines. OpenAI-compatible endpoint is a stub.
- **No feedback loop.** We can't know if memory works, if skills learn, if routing is right — because nobody is using it.

### What's UGLY

- The README comparison table claims superiority over ChatGPT/Claude/Copilot with checkmarks for features that **don't exist yet** (Web dashboard, Deep memory, Self-learns skills, Multi-tenant). This damages credibility.
- Two crates (`aggregator-core`, `aggregator-plugin`) and a 200-line PostgreSQL schema doc for a billing system with zero users. This is resume-driven development.

---

## Go/No-Go Recommendation: CONDITIONAL GO

**Don't build anything new.** The project is feature-complete for a single user.

**Immediate next 3 actions (this week):**

1. **Deploy daemon to K8s.** `kubectl apply -k k8s/overlays/prod`. Get it running.
2. **Use it.** Send messages, OCR receipts, test memory, test skills. For 7 days.
3. **Fix what breaks.** Only then decide what to build next.

**What to kill/archive:**
- `crates/ohagent-aggregator-core` + `crates/ohagent-aggregator-plugin` → move to `archive/`
- `crates/ohagent-pii-redactor` → move to `archive/`
- `crates/ohagent-infra-launcher` → move to `archive/`
- `docs/SANDBOX.md`, `docs/AGGREGATOR-DB.md` → keep as reference, mark as "future"

**What to fix in README:**
- Remove WhatsApp/Slack from architecture diagram
- Remove "Web dashboard ✅" from comparison table (0 lines of dashboard code)
- Remove "Multi-tenant ✅ day one" — it's architecturally multi-tenant but nobody else uses it
- Add "Status: pre-launch, not yet deployed"

---

## After MVP Validation (2-4 weeks)

If the 7-day test is successful:

1. **Dashboard** — React, show memory, skills, usage, costs. Actually build it (0 lines now).
2. **WhatsApp gateway** — if Sergey actually wants it.
3. **Second user onboarding** — THEN multi-tenant matters. Not before.

---

*This review is intentionally harsh. The code is good. The architecture is solid. But we're building a cathedral when we need a chapel. Ship the chapel.*
