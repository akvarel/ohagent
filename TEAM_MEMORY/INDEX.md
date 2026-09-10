# ohAgent — Team Memory

**Description:** 24/7 personal AI agent — Rust + multi-model orchestration
**Last updated:** auto

## Files

| File | Description |
|---|---|
| `INDEX.md` | ← This file |
| `SESSION_LOG.md` | Session history (auto-appended) |
| `DECISIONS.md` | Architecture Decision Records |
| `KNOWN_ISSUES.md` | Active problems |
| `ROADMAP.md` | Plans & priorities |
| `IMPROVEMENT_BACKLOG.json` | Canonical recurring-improvement backlog (machine-readable, counted) |
| `IMPROVEMENT_BACKLOG.md` | Rendered backlog view (regenerated; do not edit) |

## Recurring improvement backlog (convention)

When an agent observes a problem (bug, flake, friction, regression), it must
NOT just log it. If the same problem recurs, record or strengthen ONE entry
in `IMPROVEMENT_BACKLOG` via `ohagent_memory::backlog::ImprovementBacklog::report`:

- deterministic identity = normalized `component` + `category` + `signature`;
  exact match increments `count` and refreshes `last_seen`/evidence;
- same component+category with a different signature is linked as a
  `possible_duplicates` candidate — never silently merged;
- ranking is `priority` → `count` → `recency` (a critical one-off is never
  buried by a noisy low-impact issue);
- secrets are redacted on entry.

LLM/semantic matching may be added later as a secondary matcher; deterministic
signatures stay the identity source of record.

## Durable consolidation cursor (advance-only)

Message-log events are distilled into durable blocks by
`ohagent_core::consolidation::ConsolidationEngine` (daemon cron every 15 min):

- cursor advances ONLY in the same SQLite transaction that durably stores the
  consolidation blocks (crash/failure → events stay pending, never lost);
- if the raw log loses unconsumed events (rotation/truncation/deletion), a
  durable GAP record is written before the cursor moves past the hole;
- blocks keep provenance (source event ids); after retention removes raw
  evidence, blocks are flagged `provenance_available = false` honestly.
