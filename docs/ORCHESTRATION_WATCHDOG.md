# Orchestration Watchdog Architecture

## Status

Implemented in the maintained Jcode fork based on upstream v0.81.1. ohAgent consumes it through the pinned `jcode` submodule and public SDK runtime boundary. The watchdog does not add a second ohAgent scheduler or database.

## Placement decision

The watchdog belongs primarily in the Jcode fork, with only a thin ohAgent integration boundary.

Evidence:

- ohAgent embeds Jcode through `jcode-sdk` and launches a private runtime per tenant in `crates/ohagent-core/src/jcode_bridge.rs`.
- The active swarm and terminal/background execution paths are Jcode-owned. Jcode persists swarm plans/members in `jcode-app-core/src/server/swarm_persistence.rs`, resumes `await_members` watches in `server/comm_await.rs`, and owns background task status and delivery in `jcode-base/src/background.rs`.
- ohAgent's legacy `ohagent-swarm` tool is not registered by the SDK bridge. Reimplementing recovery in ohAgent would not observe the actual Jcode swarm/background lifecycle and would violate the runtime boundary.
- ohAgent already gives every tenant a separate Jcode home/runtime. Jcode durable state therefore remains tenant-isolated without duplicating a second ohAgent registry.

## Implemented split

### Jcode fork

- `jcode-base::orchestration_watchdog` is the durable registry and reconciliation engine.
- It stores owner/session/process/workspace, baseline and expected Git SHA, expected artifacts, deadlines, retry/backoff/model fallback policy, check leases, terminal-delivery outbox state, repository snapshots, and an audit trail.
- Background tasks register automatically. The server runs periodic reconciliation after startup and across reloads. Before orphan handling, it reads restored swarm plans and worker lifecycle state so a lost `run_plan` driver can still resolve from the real plan status.
- Reconciliation combines persisted task/process state with Git HEAD/status and artifact inspection. It never resets, checks out, cleans, kills, or rewrites a worktree.
- Swarm `run_plan` configures the watch with its workspace, deadline, expected SHA/artifacts, and retry policy. Failed drivers retry with exponential backoff and ordered model fallbacks.
- Terminal notifications are claimed durably, delivered through the existing server notification/wake path, and acknowledged only after dispatch.

### ohAgent

- ohAgent continues to own tenancy, gateway delivery, and private Jcode runtime placement.
- The parent repository advances the Jcode submodule pointer only. No duplicate watchdog database or scheduler is added to ohAgent.
- `JCODE_ORCHESTRATION_WATCHDOG_INTERVAL_SECS` can tune reconciliation frequency inside each tenant-private Jcode runtime. The default is 30 seconds.
- Docker images continue to package both `/usr/local/bin/jcode` and `/usr/local/bin/jcode-harness-api-bridge` from the same pinned submodule revision.

## Operational verification

For every submodule update that contains watchdog changes:

1. push the Jcode commit before committing the parent gitlink;
2. run Jcode formatting, code-size and test-size budgets, focused watchdog tests, and compilation for `jcode-base`, `jcode-app-core`, and the harness API server;
3. run ohAgent formatting and the focused SDK runtime, tenant-isolation, scheduler, packaging, and workspace tests;
4. verify the Docker build still contains matching Jcode CLI and API bridge binaries;
5. merge the parent change only after the submodule commit is reachable from the fork.

## Migration and compatibility

- Existing background status and swarm snapshot formats remain readable.
- New watches are created automatically for newly spawned/adopted/detached background work.
- Legacy status files without a watch retain their previous completion-delivery behavior.
- Existing `swarm run_plan` calls remain valid. New optional fields are `expected_sha`, `expected_artifacts`, `max_retries`, `retry_backoff_secs`, and `model_fallbacks`.
- Terminal watch records are retained for seven days, then removed by the watchdog. Repository and artifacts are never deleted.
