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
M0 in progress. M1-M4 pending. No completion claimed without acceptance evidence.

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
