# Verified Context Runtime

Status: experimental, feature branch only  
Validation workflow: `.github/workflows/verified-context-runtime.yml`  
Branch: `feature/verified-context-runtime`  
Base: `update/jcode-v0.90.1-20261005`

## Purpose

ohAgent already has three distinct context mechanisms:

1. Jcode owns the live execution transcript and native compaction.
2. ohAgent durable consolidation turns the message log into provenance-aware long-term blocks.
3. ohAgent rolling summary/memory layers provide compressed and retrieved history.

Verified Context Runtime adds a fourth, deliberately different layer:

**agent-maintained working context**.

The model may keep a concise representation of its current goals, decisions, active evidence references,
unresolved hypotheses, swarm status, and next actions in a normal file that it can read and edit with Jcode tools.

The design is influenced by *Context Language Models* (arXiv:2609.37725), but does not copy the paper's
"unrestricted context write" boundary. In ohAgent, editable context is a cache of working state, not authority.

The reference implementation is CC BY-NC 4.0. ohAgent does not copy its source code; this is an independent
implementation of the published architectural idea so the reference repository's non-commercial code license
does not become an ohAgent runtime dependency.

Reference:
https://arxiv.org/abs/2609.37725
https://github.com/facebookresearch/context-language-models

## Architecture

```text
Authoritative layers
-------------------------------
system/developer/user instructions
Jcode transcript
message log
durable consolidation + GAP records
memory/provenance/evidence
-------------------------------
             |
             v
   Verified Context Runtime
       accepted snapshot
             |
             v
 workspace/.ohagent-context/<session-hash>/LIVE_CONTEXT.md
             ^
             |
        agent edits
             |
             v
          edit gate
       /          \
 accepted          rejected
    |                 |
 durable snapshot     exact rollback
    |                 |
 append-only edit ledger
```

Jcode remains the execution engine. Verified Context Runtime does not replace Jcode compaction,
rewind, session persistence, tools, or provider logic.

## Turn lifecycle

For a Jcode SDK session with a workspace:

1. ohAgent initializes a session-scoped live context.
2. Before each turn, the runtime restores the last accepted snapshot if the visible file is missing or invalid.
3. ohAgent prepends a short trusted pointer telling the model where the live context is.
4. The model may read or edit the file with normal Jcode tools.
5. The Jcode turn completes normally.
6. ohAgent validates the resulting file.
7. A valid edit becomes the new accepted snapshot.
8. An invalid edit is rolled back to the exact pre-turn snapshot.
9. Accepted/rejected edits are recorded in an append-only JSONL ledger.
10. Optional native Jcode compaction can be requested after an accepted edit when an explicit token threshold is configured.

The live file contents are **not automatically injected into every prompt**. The agent reads them only when useful.

## Live context shape

Initial sections:

- Current goals
- Decisions and constraints
- Evidence references still in use
- Active hypotheses / unresolved questions
- Agent / swarm scoreboard
- Next actions

The file intentionally does not ask the model to store chain-of-thought.

## Integrity gate

Current deterministic checks:

- session-scoped path uses hashes, not raw tenant IDs;
- first protected header cannot change;
- maximum byte budget;
- optional shrink-only mode;
- NUL rejection;
- symlink replacement detection;
- directory/non-regular replacement recovery;
- basic high-confidence credential/private-key detection;
- accepted copy written atomically;
- durable edit ledger stores task hash, not raw user message;
- Unix directories/files are restricted to 0700/0600 where supported.

### Security boundary

The runtime-owned accepted snapshot and ledger live outside the agent workspace.

This is an **integrity and recovery boundary**, not a sandbox against a fully compromised process with unrestricted
filesystem access under the same OS identity. Strong hostile-agent isolation belongs in the sandbox/container layer.

Never treat LIVE_CONTEXT.md as evidence merely because it was accepted by the edit gate.

## Relationship to existing consolidation

Do not merge Verified Context Runtime with `consolidation.rs`.

They solve different problems:

```text
Durable consolidation:
"What should survive from historical source events?"

Verified live context:
"What should this agent keep immediately available while doing the current long-running job?"
```

The live context may reference durable evidence IDs, but it must not replace durable evidence/provenance.

## Configuration

### Enable/disable

```text
OHAGENT_VERIFIED_CONTEXT_ENABLED=true|false
```

Default: enabled when a Jcode session has a workspace.

### Live context budget

```text
OHAGENT_VERIFIED_CONTEXT_MAX_BYTES=65536
```

Minimum accepted value: 1024 bytes.

### Edit gate

```text
OHAGENT_VERIFIED_CONTEXT_EDIT_MODE=fit|shrink
```

- `fit`: an edit may grow while remaining under the byte budget.
- `shrink`: an edit must not increase size.

### High-pressure nudge

```text
OHAGENT_VERIFIED_CONTEXT_NUDGE_TOKENS=48000
```

Uses the latest provider-reported Jcode input-token count when available.

### Optional native Jcode compaction trigger

```text
OHAGENT_VERIFIED_CONTEXT_COMPACT_TOKENS=<token threshold>
```

Unset by default.

When set, an accepted changed live-context edit plus a sufficiently large Jcode input context can request
`JcodeClient::compact()`. This uses Jcode's native compaction implementation rather than duplicating it.

## Failure policy

Verified Context Runtime is additive and fail-open for chat availability:

- initialization failure -> ordinary Jcode session;
- snapshot/read failure -> ordinary Jcode turn;
- reconciliation failure -> response still returns, warning logged;
- invalid edit -> exact live-context rollback.

Fail-open here means the optional context optimization may disappear. It does **not** mean security/authorization gates are bypassed.

## Phase 1 implemented

- runtime-owned accepted snapshot;
- agent-visible session-scoped live file;
- append-only edit ledger;
- fit/shrink edit gate;
- protected header;
- repair/rollback;
- secret-material heuristic;
- JcodeBridge integration;
- context-pressure nudge from both Jcode input tokens and live-file occupancy;
- durable previous-rejection notice to prevent rollback/retry livelocks;
- root/coordinator context ownership rule for delegated workers;
- optional Jcode native compaction trigger;
- unit tests for edit acceptance, tamper, shrink gate, secret rejection, directory/symlink repair and metadata leakage.

## Next phase: semantic context decisions

Add a provider-neutral `ContextDecisionEngine`, using deterministic rules first and System One decisions only for
bounded semantic questions such as:

- is this item still needed for an open goal?
- is this tool output already resolved?
- is this item safe to summarize?
- must this evidence reference be preserved exactly?
- does this fact contradict an active hypothesis?
- is this item relevant to any unresolved task?
- should this state be promoted from live context to durable memory?

Candidate providers should include local/private decision models and Jev, but provider output must remain advisory.

## Multi-agent extension

Each worker should have its own live context, plus a coordinator-owned scoreboard view.

Do not give one worker unrestricted write access to another worker's accepted context.

Proposed model:

```text
coordinator LIVE_CONTEXT
worker-A LIVE_CONTEXT
worker-B LIVE_CONTEXT
worker-C LIVE_CONTEXT
        |
        v
durable swarm events / evidence
```

Shared conclusions should flow through explicit durable events/evidence, not implicit cross-editing.

## Verification roadmap

Add:

1. semantic edit verifier;
2. provenance-reference validator;
3. protected evidence-ID preservation;
4. context-diff telemetry;
5. rollback-loop detection;
6. repeated failed-edit suppression;
7. context quality benchmark;
8. optional Decision Layer integration;
9. swarm context ownership;
10. skill optimization for context-management instructions.

## Benchmark plan

Compare on real long-running ohAgent/Jcode tasks:

A. Jcode native compaction only  
B. current ohAgent rolling summary + Jcode  
C. editable CLM-style live context  
D. Verified Context Runtime  
E. Verified Context Runtime + System One semantic decisions

Measure:

- task success;
- wall time;
- input tokens;
- cache-read tokens;
- tool rounds;
- context overflows;
- compactions;
- live-context edits accepted/rejected;
- repeated work after compaction;
- evidence loss;
- incorrect stale-state retention;
- model/provider cost.

Do not claim CLM paper savings transfer to ohAgent until this benchmark is run.

## Future serving optimization

Suffix Cache Reuse from the CLM work is relevant only to self-hosted inference serving.
It should be investigated in the Jcode/local-provider serving track, not implemented in ohAgent core.
