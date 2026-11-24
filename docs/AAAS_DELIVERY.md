# ORANGEHAT-AAAS-001 delivery

## Baseline
- Repository: /sharedssd/git/orangehat/ohAgent
- Base branch: update/jcode-v1.1.2-20261123
- Base SHA: 5efd3aafaf2edf2ba2a4d8638e6e834e3a093595
- Delivery branch: feature/orangehat-aaas-001
- Preserve unrelated TEAM_MEMORY/SESSION_LOG.md change and jcode submodule.
- Specification: Google Drive document 1OO-cXqhhIhBqi7c9AwIpOtvDAHrvdwySmp4wf-A3a8E, version 1.0, 2026-11-24.
- Owner clarification: ohAgent is the runtime for Standard Agent Specification, composing domain-specific agents into a universe. Never confuse a workflow stage, definition and actual runtime instance.

## Milestones
| Milestone | Status | Evidence / remaining gate |
| --- | --- | --- |
| M0 | PASS | Repository baseline, existing runtime/auth mapping, typed catalog and bounded pilot contract recorded. |
| M1 | BLOCKED for staged delivery | Site/product/demo implemented and browser-tested EN/LV/RU. Approved staging origin and verified company contact are missing. |
| M2 | BLOCKED for customer acceptance | Actual Jcode/provider produces separate verified results for two private test principals with distinct goals/materials. Deployed OIDC and real browser sign-in are not established. |
| M3 | Partial, not accepted complete | Fixed four-role fork/join, atomic journal, cursor recovery, restart fencing and actual logical SDK cancellation tested. Retry/approval continuations are not supported by this profile. |
| M4 | BLOCKED | Feature branch and draft PR published. Local checks and labelled recording exist. Full staging identity/revocation/reconnect demonstration and remaining release gaps are open. |

## Initial evidence
- Rust JcodeBridge uses public jcode-sdk and private runtime homes per tenant, not a separate web executor.
- Existing dashboard is React 18/Vite 5, with legacy operator routes and no client login found in its entry point.
- Existing /api middleware uses an operator API key, not tenant membership. Do not expose it as customer login.
- No Standard Agent Specification implementation found by initial crates search. Compatibility must be verified, not advertised as certified.
- orangehat.ai DNS resolves through Cloudflare, HTTPS returned 503 at 2026-11-24T20:55Z. No DNS or production mutations authorized.
- Graphify queried before source inspection. Existing index is stale relative to baseline, branch hook rebuild initiated automatically.

## Safety and acceptance
Keep existing engine, provider routing, secret policy and legacy routes. No production, DNS, secret, paid service or main/master changes. User-facing copy EN/LV/RU. Runtime data server-scoped by authenticated principal, never browser tenant_id. Real happy-path evidence required for A03/A04/A13. Fixtures explicitly labelled. Full criteria A01-A18 tracked in final acceptance matrix. Staging is not production.

## Delegation routing
Catalog verified with `jcode model list -p openai`: gpt-6.1-sol is present. Authorized automatic first route: openai:gpt-6.1-sol. Tool swarm list_models output exceeded tool context guard, and spawn schema has no model selector, so explicit CLI workers use `jcode run -p openai -m gpt-6.1-sol` to avoid an implicit inherited route. Catalog presence is not proof of successful inference.

## Integration map and final local evidence

| Existing component | Confirmed boundary | Minimal integration |
| --- | --- | --- |
| React/Vite dashboard | Legacy operator routes and API-key access | Add public routes and lazy PKCE customer subtree, preserve existing pages. |
| JcodeBridge/public SDK | Real per-tenant child runtime and sessions | Dedicated deny-all-tools customer bridge, independent SDK attachment per invocation, acceptance/cancel barrier and guarded cleanup. |
| Existing daemon | Messaging/scheduler plus operator-key router | Merge tenant AaaS outside operator middleware; add safe `--site-only` and explicit static document routes. |
| Standard Agent package | Pinned official definitions and provider factory types | Strict trusted build-time text-profile export; no arbitrary package execution or second model/router. |
| Durable workflow state | No prior customer AaaS journal | Add tenant-scoped SQLite snapshots/journal in one transaction, immutable process versions, idempotency and producer fencing. |

Production code checked: `8a9b26e3c2e2a761c5d79e769115ff31f48e9da3`.
Draft PR: https://github.com/akvarel/ohagent/pull/8.

Actual Jcode/provider checks passed for two private test identities, distinct caller goals/materials, four roles each and overlapping analyst/evidence-checker branches. Both artifacts passed required-field and exact-quotation verification. Actual logical SDK cancellation also passed. This does not establish deployed customer authentication or physical provider termination.

Final local checks: 19 backend regressions, 2 separately opted-in actual-runtime tests, 6 verifier tests, core policy/boundary 2 + 7, 11 protocol tests, 3 official package tests/typecheck, strict changed-crate Clippy, daemon build and 41 compiled-site browser scenarios. Four actual same-origin HTTP tests passed. The complete repository strict lint remains blocked by existing unrelated dependency lints.

See `AAAS_ACCEPTANCE.md` for every exact A01-A18 PASS/FAIL/BLOCKED row and evidence provenance, `AAAS_RUNBOOK.md` for launch/schema/staging/rollback, and `evidence/aaas-001` for real sanitized traces, results and clearly labelled component recording. The recording is not a 60–120 second live authenticated workflow. Independent review reports remain historical evidence, not blanket approval of later work.

No production application deployment, DNS/credential mutation or Jcode submodule change occurred. Unrelated `TEAM_MEMORY/SESSION_LOG.md` remains untouched. Full task acceptance is still open, including retry attempts, monetary usage/budget controls, live membership revocation, AaaS monitoring and staged browser/performance verification.

Follow-up on November 24, 2026: the owner assigned staging to `stage.orangehat.ai`, future production to `orangehat.ai`, and explicitly authorized both dedicated public Authentik registrations. `ohagent-stage` and `ohagent-prod` now exist in the existing BugZero identity service, without modifying BugZero clients or creating client secrets. Live discovery/JWKS/CORS, callback rejection, installed S256 policy evaluation and actual local daemon discovery passed. The backend access-token profile now rejects same-audience ID-token substitution (RED/GREEN regression); 20 backend regressions passed. See `AAAS_OIDC.md` and `evidence/aaas-001/oidc-registration.json`. This is not a deployed staging website or two-user live login acceptance. Remaining owner inputs include staging provider-credential storage/provisioning choice, authorized real-user acceptance and verified contact/legal data.
