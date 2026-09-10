# Improvement Backlog

> Generated from `IMPROVEMENT_BACKLOG.json` — do not edit by hand;
> report occurrences via `ImprovementBacklog::report` (crates/ohagent-memory/src/backlog.rs).

| id | priority | count | component | category | status | summary | last_seen |
|---|---|---|---|---|---|---|---|
| IMP-0001 | high | 1 | jcode | consolidation-gap | open | Consolidation cursor: events could silently vanish between message log and durable memory; fixed with advance-only cursor + GAP records (see ohagent-core/consolidation.rs) | 2026-09-03 |
| IMP-0002 | high | 1 | ohagent | recurring-pain | open | Repeated problems dissolved into logs; fixed with counted IMPROVEMENT_BACKLOG convention | 2026-09-03 |
| IMP-FUT-1 | low | 1 | jcode | scheduler | open | FUTURE-1 (backlog only): adaptive background wakeup (brewing → shorter, quiet → longer, hard clamps, failure backoff; explicit user schedules never modified) | 2026-09-03 |
| IMP-FUT-2 | low | 1 | ohagent-skills | evolution | open | FUTURE-2 (backlog only): verified skill evolution gate (proposal → staging → deterministic validation → probes → regression corpus → optional LLM judge → promotion; BugZero adds known faults / false-PASS suite / GVR) | 2026-09-03 |
