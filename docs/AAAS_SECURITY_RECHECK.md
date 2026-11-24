# ORANGEHAT-AAAS-001 independent changed-evidence security recheck

Review window: 2026-11-24 21:17:23 through 21:19:41 UTC. This is the single bounded changed-evidence retry, not implementation or final acceptance. Read `AGENTS.md`, the unchanged prior `docs/AAAS_SECURITY_REVIEW.md`, the integration contract, and current source. Sole authorized source-tree write: this report. No source edits, commits, pushes, agents, configuration changes, secrets, providers, production, or real Jcode execution.

## Disposition

- **R1 addressed in current source and passing synthetic regression evidence.** Snapshot watermark no longer acknowledges unread SSE events after the initial baseline. Terminal snapshots must drain through their watermark.
- **R4 addressed for the declared trusted text-profile exporter.** Strict key allowlists reject the previously erased model semantics, with independently passing rejection checks.
- **R2 materially mitigated, with medium lifecycle/availability residuals.** Retained runtime count and durable process-version/run counts are now bounded. Cleanup and creation remain cancellation-sensitive, and the one-connection-per-tenant attachment model is not safe for unrestricted overlapping sessions.
- **R3's original acknowledgment-only confirmation is corrected for an already-started turn.** The adapter now waits for the SDK run future. There are still medium cancellation lifecycle residuals below. No claim of engine termination under every SDK/server fallback is justified.
- **No real-runtime or OIDC E2E certification.** Backend tests were independently attempted but blocked by the shared Cargo build lock. Prior passing results are historical evidence, not this review's result.

## Exact commands and observed results

From repository root:

```sh
node --experimental-strip-types --test agents/standardagent-analytical-brief/index.test.ts crates/ohagent-dashboard/src/aaas/protocol.test.mjs
timeout 30s cargo test -p ohagent-aaas
```

The two commands ran sequentially in one shell background task, started 21:17:35 UTC and completed 21:18:06 UTC. Node: **14 tests passed, 0 failed**, duration 593.54 ms. This comprises 3 package tests and 11 protocol tests, including unread-journal preservation and unsupported-model rejection. Cargo: **exit 124**, with only `Blocking waiting for file lock on build directory`. No Rust test execution or compilation success was observed. No second attempt, alternate target build, lock deletion, or wait beyond the requested 30-second bound.

Additional independent in-memory exporter checks, exit 0, output `Independent R4 nested/agent/prompt/model strict-key checks: 4 rejected`:

```sh
node --experimental-strip-types --input-type=module <<'NODE'
import assert from 'node:assert/strict';
import {agents,prompts,models,exportTextProfile} from './agents/standardagent-analytical-brief/index.ts';
const cases = [[agents,'coordinator',{effects:[]}],[prompts,'coordinator_prompt',{providerOptions:{}}],[models,'deepseek_flash',{providerTools:['web_search']}],[models,'deepseek_flash',{capabilities:{supportsToolCalls:false,supportsImages:false,unsupported:true}}]];
for(const [registry,key,extra] of cases){const original=registry[key];try{registry[key]=async()=>({...await original(),...extra});await assert.rejects(exportTextProfile,/unsupported_standard_agent/)}finally{registry[key]=original}}
console.log('Independent R4 nested/agent/prompt/model strict-key checks: 4 rejected');
NODE
```

No provider factory was invoked by these probes. The original registry factories were restored in memory. No test files were changed. Initial `rg` inspection commands could not run because `rg` is unavailable. Inspections continued with `grep`, `sed`, and `nl`. These are inspection limitations, not test failures.

## Requirement-to-evidence assessment

### R1: consumed SSE cursor

`crates/ohagent-dashboard/src/aaas/protocol.ts`, `acceptSnapshot`, preserves `state.cursor` once a snapshot exists, while rejecting backward versions/watermarks. The initial snapshot intentionally establishes a baseline. `useRunJournal.ts` checks both terminal status and `state.cursor >= state.snapshot.watermark` after a chunk and after stream closure. A snapshot fetched after event 1 cannot silently consume committed but unread events 2..4. Explicit reset/gap recovery still advances to a fresh baseline and clears history, which is distinct from normal delivery and is visible as a reset. Passing Node test `newer snapshots never skip unread journal events` covers the reducer consequence. No mounted React hook or browser timing check was executed.

### R2: caps, serialized launch, cleanup and transactional quotas

`crates/ohagent-core/src/jcode_bridge.rs:498-565`: fast lookup is followed by a write-lock lookup, then restricted-runtime `len() >= 16` rejection before launching. Holding the write guard over launch serializes successful cold starts and prevents the previous ordinary duplicate-launch race. The cap applies only to the dedicated text-only bridge. There is no observed recursive acquisition or lock-order cycle in this path. However, a slow launch holds the global runtime write lock, blocking even reads for established tenants until it finishes or its caller is cancelled. This is head-of-line blocking, not evidence of a permanent deadlock.

`crates/ohagent-aaas/src/store.rs`, `process`, `start`, and `check_capacity`: `TransactionBehavior::Immediate` covers both quota checks and inserts. Process **rows**, including immutable versions, are capped at 256 per tenant / 4096 global. Runs are capped at 100 per tenant / 1000 global. Existing idempotency entries are checked before the run quota, so a repeat does not require fresh capacity. These are genuine finite cumulative bounds, not merely simultaneous-work limits. Source test `pilot_quotas_bound_retained_versions_and_runs_without_breaking_retries` exercises per-tenant boundaries and repeat behavior, but it was not executed by this reviewer. Global boundary and concurrent real-runtime behavior remain untested here.

`crates/ohagent-daemon/src/aaas.rs:21-40,79-87`: `ActiveSession` schedules cancel when unfinished, then detach, then local-map removal. `drop_session` removes both session and verified-context handles. Progress storage independently has a 128-entry cap (`crates/ohagent-progress/src/lib.rs`), so its retained entries are not a new unbounded map.

**Medium residual R2-L1, creation/cleanup ownership:** the 30-second `create_session` timeout precedes guard construction. Bridge creation awaits blocking SDK creation, model selection, and optional verified-context initialization. Timing out after SDK session creation but before returning the handle can leave a real session without this guard. Cancellation while cold launch is in `spawn_blocking` releases the map lock without cancelling that launch. Its eventual unclaimed `LaunchedInstance` result is dropped and has shutdown cleanup, so a permanent child leak is not established, but transient duplicate launches outside the retained-map cap remain possible. The cap is a retained-runtime cap, not a proven cap on all in-flight child processes. Cleanup after a guard exists is detached best-effort work, not awaited before the backend releases the run permit, and runtime shutdown may prevent that cleanup task completing. This report does not claim a measured leak or load-induced denial of service.

**Medium residual R2-L2, shared attachment lifecycle:** every session of a principal clones the same `TenantRuntime.client` (`jcode_bridge.rs:327-339,362-365`). The SDK explicitly documents one attached session per connection (`jcode-sdk/src/client.rs:695-700`). Harness `create_session` changes that attachment; requests for a different session are rejected (`jcode-harness-api-server/src/translate.rs:390-409`). The API permits overlapping runs for one principal, with no per-principal session-lifetime serialization. Thus creation of B while A runs can make A cancellation/detach fail with `UnknownSession`, and completion events can no longer be assumed to reach A's session-scoped subscription. Detached guard cleanup also overlaps the next stage's creation. This is an availability/cancellation correctness issue within one principal, not a demonstrated cross-tenant access bypass. Cold-launch serialization does not serialize attached session lifetimes. No live interleaving was executed.

Finite lifetime caps have no eviction/reclamation policy: 16 previously admitted subjects can occupy retained runtimes until restart, and cumulative quotas eventually refuse new work. This is a bounded-pilot operational limitation, not the former unlimited-retention claim.

### R3: SDK run and cancellation semantics

Source trace:

1. `SessionHandle::send_message_with_images` starts a blocking SDK run when its async future is polled (`jcode_bridge.rs:77-119`). Dropping its awaiting future does not abort the blocking operation.
2. `JcodeClient::cancel` waits for request OK (`jcode-sdk/src/client.rs:932-937`). Harness translates Cancel to a legacy cancel and its Ack to OK (`translate.rs:680-683,2501-2526`). Server queues that Ack before awaiting `cancel_processing_message` (`client_lifecycle.rs:863-899`). This is still request acceptance, not termination.
3. SDK `run` subscribes before sending the message. It records `TurnStopped` but returns success only on `TurnDone`; `Error` or disconnect returns an error (`client.rs:1587-1673`). `wait_for_turn_stop` accepts only successful future resolution within 10 seconds, so an Error/disconnect cannot itself confirm cancellation.
4. Adapter deadline and cancellation branches now await that same pinned turn after cancel acknowledgment (`aaas.rs:105-119`). Failure exits without `confirm_cancelled`. Backend converts a remaining `cancellation_requested` status to `interrupted` on producer exit (`ohagent-aaas/src/http.rs:143-155`). The original early-confirmation path for an active turn is therefore removed.
5. Harness emits `TurnDone` for the completed message's `Done`, excluding known control completions (`translate.rs:2006-2045`). The idle pre-turn cancellation branch (`aaas.rs:84-87`) has no message from this newly created session in flight, so it is not by itself the original active-turn defect.

**Medium residual R3-L1, cancel-before-start race:** `turn` is lazy, and the first interval tick is immediately ready. A cancellation committed after `start_instance` but before the first `select!` can select the cancellation branch before polling `turn`. The adapter cancels an idle session, then `wait_for_turn_stop(&mut turn)` polls the still-unstarted future and launches the message *after* cancellation acknowledgment. Even if the turn branch was polled first, `spawn_blocking` scheduling allows a similar cancel-before-send ordering. A short turn can finish and be reported cancelled despite having performed inference after cancel. A slow one times out unconfirmed and gets best-effort second cancellation from the guard. This is source-derived reachability, not an executed race reproduction. The new oneshot unit test checks waiting for a future, not ordering against actual message acceptance, so it does not cover this gap. A start/acceptance barrier or equivalent lifecycle serialization is needed to establish cancellation targets the submitted turn.

**SDK termination limitation:** `client_processing.rs:576-641` waits 500 ms cooperatively, aborts on timeout, waits up to another 2000 ms, then still emits the message `Done` even if abort release timed out. Therefore SDK `TurnDone` is the supported logical turn barrier, not an unconditional proof every underlying task/provider operation has physically terminated. The new adapter respects that SDK barrier, but this review cannot certify stronger termination semantics. No new high-severity unconditional Ack-only cancellation finding is asserted for the changed active-turn path.

### R4: strict exporter rejection

`agents/standardagent-analytical-brief/index.ts`, `onlyKeys` and `exportTextProfile`, checks top-level registries and exact allowed keys on agents, both sides, prompts, models, and capabilities. In particular `providerTools`, `providerOptions`, `fallbacks`, and unknown future model fields now throw instead of disappearing. Required bounded values are separately checked, including `stopOnResponse`, max steps/turns, model/provider identity, and false tool/image capability flags. Existing package tests and the independent four probes above pass. This closes the reported field-erasure gap for trusted build-time definitions. It is not arbitrary hostile-JavaScript/package validation or complete specification conformance.

## Limitations and closure

Source-only findings distinguish reachability from measured runtime behavior. No daemon tests, live SDK turn, malicious-materials tool denial, cancellation timing run, browser/OIDC exchange, or deployed acceptance was executed. No paid calls or external provider requests occurred. No broad source or submodule mutation was performed. No permanent lock cycle was found in inspected changed paths, but unbounded SDK event waits and shared attachment races prevent a general deadlock/lifecycle clearance.

Prior report preservation was checked before and after this report was written using `git hash-object docs/AAAS_SECURITY_REVIEW.md`: `fd2789d6bc4563ab9b9ece11f9bd33eabd8c08e2`. `git diff --no-index --check /dev/null docs/AAAS_SECURITY_RECHECK.md` exited 0 with no whitespace errors. The prior report remains historical and unchanged. This report does not supersede its limitations or authorize deployment. Root retains final acceptance and integration ownership.
