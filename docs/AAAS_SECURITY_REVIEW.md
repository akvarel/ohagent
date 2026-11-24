# ORANGEHAT-AAAS-001 independent acceptance security review

Review started 2026-11-24 21:08 UTC. Independent reviewer, not implementation author. Root owns integration and final acceptance. Source is read-only and this report is the sole authorized write. No production, credentials, submodule or Git mutations authorized or performed.

## Scope and disposition at 21:13 UTC

Read `AGENTS.md`, `docs/AAAS_CONTRACT.md`, and `docs/AAAS_DELIVERY.md`. Reviewed backend auth/router/store/sink, core text-only bridge and brief verifier, daemon adapter/static routes, browser auth/journal/client and public copy. Assessing the explicit supported text-profile pilot, not arbitrary Standard Agent Specification conformance. **Do not infer real-runtime acceptance from the passing fixture suites.** Independent disposition: acceptance is not established. R3 is an open high-severity lifecycle contract violation, R2 remains a medium resource-retention risk, R4 is an open medium supported-profile fail-closed gap. R1 was addressed concurrently and its reducer interleaving retest passed. No source fixes were authored by this reviewer.

## Independently executed validation

| Exact command (repository root) | Actual result | Evidence scope |
| --- | --- | --- |
| `cargo test -p ohagent-aaas` | Exit 0, 12 passed, 0 failed, 0 doc tests. Completed 21:08:13 UTC, 0.92 s wall time, test execution 0.50 s. | Actual axum router, on-disk temporary SQLite, generated ephemeral signed JWT fixtures. Runtime/identity injection is test-only. Not Jcode execution or deployed OIDC. |
| `node --experimental-strip-types --test crates/ohagent-dashboard/src/aaas/protocol.test.mjs` | Exit 0, 10 passed, 0 failed. | Synthetic reducer/SSE/parser/return-route fixtures. No React hook integration, browser OIDC exchange or live API. |

Protocol command was run twice (first combined output was truncated, second captured explicit counts). No tests were changed or weakened. Backend run only once. Existing shared target cache was used, not installed or removed. No build/update installation occurred.

## Findings (source-confirmed unless otherwise stated)

### R3: High, cancellation is marked confirmed on request acknowledgment, before engine termination

- Locations: `crates/ohagent-daemon/src/aaas.rs:75-78,98-106`; `crates/ohagent-core/src/jcode_bridge.rs:142-149`; read-only dependency trace `jcode/crates/jcode-sdk/src/client.rs:932-936`, `jcode/crates/jcode-harness-api-server/src/translate.rs:680-683,2501-2526`, `jcode/crates/jcode-app-core/src/server/client_lifecycle.rs:863-899`, `jcode/crates/jcode-app-core/src/server/client_processing.rs:554-617`.
- `session.cancel()` waits for SDK request OK, and the adapter immediately calls `confirm_cancelled()`. The actual server queues Cancel Ack before calling the cancellation handler. Harness translation converts that Ack into API `Ok`. Engine cancellation can then wait 500 ms for cooperation and up to another 2 s after abort. The API acknowledgment therefore is not engine termination confirmation. The adapter also drops the outstanding turn future rather than awaiting its completion. Its `spawn_blocking` SDK work is not cancelled by dropping the async join handle.
- Reproduction recipe: run a slow text turn, POST cancellation with the current version, and delay engine cooperative termination while allowing the Cancel Ack through. The adapter may persist `run.cancelled` and release the run permit while the engine is still processing. This violates the contract requirement to remain `cancellation_requested` until the engine confirms cancellation. No paid/provider invocation was performed. This finding is established by the local SDK/server control-flow trace, not a measured live timing claim.
- Fix direction for root: await a terminal/idle condition tied to the specific runtime turn, with a bounded fallback to interrupted/unconfirmed. Do not treat request acceptance or the early `TurnStopped` notification (also published before cancellation is signalled) as termination proof. Producer fencing already prevents late journal writes, but does not terminate external work.

### R4: Medium contract gap, build-time text exporter silently drops unsupported model capabilities

- Location: `agents/standardagent-analytical-brief/index.ts:49-64` (`exportTextProfile`), with runtime deserialization at `crates/ohagent-core/src/standard_agents.rs:23-34`.
- The exporter checks `prompt.tools`, provider identity and model name, but does not reject `model.providerTools`, `model.providerOptions`, model fallbacks and other unsupported model semantics. A profile can therefore be emitted successfully after an unsupported standard feature is added. Rust `deny_unknown_fields` cannot detect features already erased by export.
- Independently reproduced using an in-memory factory override only: preserve the original `models.deepseek_flash` factory, replace it with an async wrapper adding `providerTools:['web_search']` and `providerOptions:{reviewFixture:'unsupported'}`, call `exportTextProfile()`, and assert deep equality with the baseline profile. Exit 0 and output confirmed unsupported fields were accepted and silently omitted. No package/source/profile file was changed and no provider was constructed or called.
- This is not a current arbitrary-package upload exploit. The checked-in trusted package currently has no such tools. It is a failure of the explicit contract that unsupported standard fields fail closed, relevant to maintaining even this bounded supported profile. Root should reject unsupported keys or tightly validate a declared supported subset rather than claim general conformance.


### R1: Medium at initial review, concurrent fix observed and targeted retest passed

- Locations: `crates/ohagent-dashboard/src/aaas/useRunJournal.ts:29-35,80-100`, `crates/ohagent-dashboard/src/aaas/protocol.ts:19-22,31-35`.
- Initial behavior (before concurrent integration edit at approximately 21:11 UTC): on each received chunk the hook fetched a snapshot. `acceptSnapshot` advanced the same cursor used for journal dedup to that snapshot watermark. Events already committed but not yet read from the open stream are then rejected as old. The snapshot can also be terminal and cause immediate stream cancellation before its remaining events are delivered. State remains authoritative, but the live event history is incomplete and fresh dispatch pulses can disappear.
- Reproduction recipe: deliver event 1 to the hook, then commit events 2..4 before its follow-up GET snapshot completes. Return snapshot watermark 4, then deliver stream events 2..4. The reducer drops all three. With terminal snapshot 4, hook returns without consuming them. Existing pure reducer tests intentionally accept snapshot cursor advancement and do not test this hook race.
- Independently reproduced with a synthetic reducer interleaving: event 1, snapshot watermark/version 4, events 2..4 leaves cursor 4 but `events.map(e => e.run_seq)` equal to `[1]`. Node assertions passed, exit 0. This reproduces the reduction consequence, not a mounted React/browser timing test.
- Current disposition at 21:11:57 UTC: external integration changes now preserve the consumed journal cursor after initial snapshot and require `cursor >= snapshot.watermark` before terminal stream closure (`protocol.ts:20-21`, `useRunJournal.ts:93,100`). A changed-evidence targeted retest with initial snapshot 0, event 1, snapshot 4, events 2..4 passed and retained `[1,2,3,4]`. This addresses the identified race in source and reducer evidence. The complete frontend suite was not rerun after that concurrent edit, and no mounted-hook/browser test was executed.

### R2: Medium, bounded active jobs do not bound retained runtimes/sessions/storage

- Locations: `crates/ohagent-core/src/jcode_bridge.rs:172-175,349-358,415-434,494-555`; `crates/ohagent-daemon/src/aaas.rs:51-61,98-105`; `crates/ohagent-aaas/src/store.rs:21-27,75-83,192-200,383-390`.
- Initial behavior: the dedicated customer bridge retains a runtime per signed principal indefinitely. Successful role completion called SDK detach but did not remove local `sessions`/`verified_contexts`; error/cancel paths also lacked detach/drop. At 21:11:57 UTC, a concurrent integration change introduced an `ActiveSession` drop guard that asynchronously cancels, detaches and drops local session handles (`aaas.rs:12-30,74`). This improves per-session cleanup and is not reviewer-authored. It has not been runtime-tested here. The runtime map still retains one runtime per principal indefinitely, and workspaces, process versions, journal entries and idempotency rows still have no retention/quota. Eight runtime slots constrain concurrent requests, not cumulative process count, memory or disk. The 30-second `create_session` timeout also occurs before the session guard exists, while SDK creation uses non-abortable `spawn_blocking`.
- Reproduction recipe for the remaining risk: use many valid OIDC subjects to grow retained runtime entries, and repeatedly update saved processes to accumulate up to 100 KB per immutable version without starting inference. For the earlier per-session map leak, repeated runs/cancellations were the source-derived reproduction recipe, but the concurrent guard change now requires runtime revalidation rather than assuming that leak remains.
- Impact: authenticated resource exhaustion, potentially affecting other principals. No destructive load test or actual multi-user Jcode process test performed. Retention of transcripts can be intentional, but unlimited live runtime/map retention is distinct from durable diagnostics.

## Positive evidence and boundaries

- Tenant scope is server-derived as SHA256(verified issuer + NUL + signed subject). One workspace per principal is explicitly documented, not organization membership. Store ownership predicates include tenant for run/process/artifact/journal/idempotency access. Unknown tenant fields fail closed on mutation inputs. No concrete cross-principal read/write bypass found in inspected paths.
- JWT backend validates asymmetric signature, issuer, audience, expiry, nbf, subject and signing-key intent. Discovery uses HTTPS, no redirects, bounded bodies and timeouts. Missing config/runtime fails closed. AaaS router is merged outside operator-key middleware.
- Atomic SQLite updates protect snapshots, idempotency claims and journals. Producer epochs, cancellation fences and terminal guards prevent late mutation. Replay only reads stored events. Restart interrupts incomplete runs instead of rerunning them. Single active database owner is an explicit deployment assumption.
- Browser PKCE uses cryptographic state/nonce/verifier, S256, exact callback, one-use transaction deletion, restricted same-origin return routes and signature-verified ID token. Tokens are not placed in URLs. Actual IdP interoperability remains untested.
- Verification is explicitly structural and exact-source-quotation checking, not proof of factual truth. Download is an authorized Markdown attachment with nosniff. Arbitrary model-generated Markdown is not rendered as HTML by the inspected client.
- Static text-only enforcement trace: the bridge supplies `JCODE_TOOL_PROFILE=none`, empty `JCODE_TOOLS`, `JCODE_DISABLE_BASE_TOOLS=true` and disables codemode (`jcode_bridge.rs:518-526`). The dependency applies environment tool overrides (`jcode-base/src/config/env_overrides.rs:142-154`), computes an empty allowed set (`jcode-base/src/config.rs:944-954`), and supplies it to agent construction (`jcode-app-core/src/agent.rs:590-602`). This supports the design but does not certify the installed external binary or a real malicious-materials turn. Existing `aaas_runtime_policy.rs` tests only check the bridge flag, not actual tools available in a launched runtime.
- Public copy explicitly disclaims verified standards conformance and labels the demonstration as non-runtime evidence. This is appropriate, not a reason to require full standard conformance for the supported text pilot.

## Public supported-profile disclosure limitation

Low-severity contract/expectation gap: `crates/ohagent-dashboard/src/public/locale.tsx:27-35` and `src/public/Product.tsx:6-10` describe a workspace for defining specialist agents, but do not explicitly name the currently available supplied-text analytical-brief pilot or its no-tools, fixed-three-role sequential scope. The demo and standards disclaimers are honest, and the client itself has clearer limits. Still, the contract asks for pilot capabilities and limitations to be explicit on the landing. A short localized supported-profile notice would distinguish current capability from intended runtime positioning without requiring arbitrary-package/full-standard support. This is a copy gap, not a proven runtime exploit.

## Exact supplemental synthetic checks

All three heredocs below ran from the repository root with `node --experimental-strip-types --input-type=module`. They do not write files or call a model. First check reproduced the original reducer defect, exit 0:

```sh
node --experimental-strip-types --input-type=module <<'NODE'
import assert from 'node:assert/strict'
import {newJournal,acceptEvent,acceptSnapshot} from './crates/ohagent-dashboard/src/aaas/protocol.ts'
const snapshot = {id:'review-fixture',version:4,watermark:4,status:'running',tasks:[],edges:[]}
const event = n => ({schema_version:1,event_id:`review-fixture-${n}`,run_id:'review-fixture',run_seq:n,type:'task.dispatched',recorded_at:'2026-11-24T21:00:00Z',occurred_at:'2026-11-24T21:00:00Z',payload:{}})
let state = acceptEvent(newJournal('review-fixture'),event(1))
state = acceptSnapshot(state,snapshot)
for (let n=2;n<=4;n++) state=acceptEvent(state,event(n))
assert.equal(state.cursor,4)
assert.deepEqual(state.events.map(e=>e.run_seq),[1])
console.log('R1 reproduced with synthetic reducer interleaving: cursor=4, retained journal sequences=[1], sequences 2..4 skipped')
NODE
```

Exporter check, exit 0, accepted the unsupported additions and returned an identical profile:

```sh
node --experimental-strip-types --input-type=module <<'NODE'
import assert from 'node:assert/strict'
import {models,exportTextProfile} from './agents/standardagent-analytical-brief/index.ts'
const baseline = await exportTextProfile()
const original = models.deepseek_flash
models.deepseek_flash = async () => ({...await original(),providerTools:['web_search'],providerOptions:{reviewFixture:'unsupported'}})
const changed = await exportTextProfile()
assert.deepEqual(changed,baseline)
console.log('R4 reproduced in memory only: unsupported providerTools/providerOptions accepted and silently omitted from exported text profile')
NODE
```

One changed-evidence retry of the R1 interleaving check was executed after the concurrent fix, exit 0:

```sh
node --experimental-strip-types --input-type=module <<'NODE'
import assert from 'node:assert/strict'
import {newJournal,acceptEvent,acceptSnapshot} from './crates/ohagent-dashboard/src/aaas/protocol.ts'
const snapshot = n => ({id:'review-fixture',version:n,watermark:n,status:'running',tasks:[],edges:[]})
const event = n => ({schema_version:1,event_id:`review-fixture-${n}`,run_id:'review-fixture',run_seq:n,type:'task.dispatched',recorded_at:'2026-11-24T21:00:00Z',occurred_at:'2026-11-24T21:00:00Z',payload:{}})
let state = acceptSnapshot(newJournal('review-fixture'),snapshot(0))
state = acceptEvent(state,event(1))
state = acceptSnapshot(state,snapshot(4))
assert.equal(state.cursor,1)
for (let n=2;n<=4;n++) state=acceptEvent(state,event(n))
assert.deepEqual(state.events.map(e=>e.run_seq),[1,2,3,4])
console.log('R1 changed-evidence retest passed: snapshot4 preserves consumed cursor1 and journal drains through [1,2,3,4]')
NODE
```

## Limitations and review closure

Final source spot-check at 2026-11-24 21:12:50 UTC confirmed the R3 acknowledgment-based confirmation and R4 exporter checks remained unchanged. `git diff --no-index --check /dev/null docs/AAAS_SECURITY_REVIEW.md` completed with exit 0 and no whitespace errors. This report is the only source-tree file written by this reviewer. Existing unrelated work and all concurrent integration changes were preserved. No commit or push was made, per the packet. No child agents were spawned.

Eight-minute bounded review against a changing integration workspace. No commits, pushes, staging, secrets, production or submodule changes. No real Jcode invocation or provider call was authorized/executed here, no live OIDC configuration was supplied, and no end-to-end A03/A04/A13 acceptance is established. Full standards conformance, arbitrary npm packages, hooks, effects, handlers and parallel workflow execution are outside the supported profile. The adapter is sequential and does not fake parallel execution. The backend supports a configured API JWT audience, while browser login requests only `openid profile` and exposes no API resource/audience parameter. This can work with an IdP/client configured to issue the required default API audience, but must be demonstrated against the chosen IdP. ID-token algorithms in the browser are limited to RS256/ES256 even though backend API JWT verification supports more algorithms. No unsupported deployment compatibility is claimed.
