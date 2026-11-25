# Trusted analytical brief Standard Agent package

This private package declares four domain-specific agents using the pinned official `@standardagents/spec` and `@standardagents/openai` 0.45.13 types, targeting specification 0.2.0.

```text
coordinator -> analyst ---------> reviewer
            -> evidence_checker ->
```

The two middle roles execute concurrently in distinct actual Jcode sessions. A role definition is not a runtime instance. The final reviewer merges both branch outputs with the original caller material. ohAgent verifies required JSON fields and exact source quotations, then creates Markdown. This structural check does not certify semantic truth.

## Supported boundary

`export.ts` evaluates only this trusted build-time package, rejects unsupported fields, and produces deterministic `text-profile.json`. Rust loads that checked-in profile. The daemon does not evaluate caller-supplied JavaScript, download arbitrary npm packages or implement a second agent execution engine. All actual inference remains through JcodeBridge/public SDK. The dedicated customer bridge enforces an empty tool set and disables inherited logins.

Supported: named definitions, declared prompts, one bounded text model turn per invocation, a fixed fork/join DAG, exact-source quote verification. Unsupported: arbitrary tools, hooks, effects, handlers, provider options/fallbacks, interactive approval continuations, uploads and arbitrary workflow graphs. This is a bounded compatibility profile, not full Standard Agent Specification certification.

## Reproduce

```sh
npm ci
node --test index.test.ts
node export.ts --check
# Requires the existing dashboard TypeScript development dependency.
../../crates/ohagent-dashboard/node_modules/.bin/tsc --noEmit --target es2022 --module nodenext --moduleResolution nodenext --allowImportingTsExtensions --skipLibCheck index.ts
```

After an intentional definition change run `node export.ts`, inspect the generated diff and rerun Rust verifier/runtime checks. Model configuration is `deepseek:deepseek-v4-flash`, using only an already-authorized provider environment. No credential is shipped in this package.
