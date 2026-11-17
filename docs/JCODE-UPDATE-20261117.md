# Jcode v1.0.0 integration

Date: 2026-11-17.

## Integration

The `jcode` submodule follows `update/jcode-v1.0.0-20261117` in the OrangeHat fork. This branch merges stable upstream v1.0.0 while preserving the fork's external memory, deferred MCP discovery, orchestration watchdogs, routing integrations, and team-memory policies. The fork records merge decisions and acceptance findings in `jcode/docs/FORK_UPDATE_V1.0.0.md`.

Jcode v1 uses shared message content. Parent message constructors convert owned block vectors through `.into()` without changing roles, block contents, timestamps, or provider behavior.

Jcode also requires rusqlite 0.41. All parent SQLite consumers now inherit that version from the workspace, avoiding incompatible native SQLite links. Keep `bundled`, `blob`, and `fallible_uint` enabled explicitly. The unsigned conversion feature is required by isolated consumers, even when a whole-workspace build happens to enable it transitively.

## Validation

- `cargo check --locked --workspace --all-targets` passed against final fork commit `1513367cd`.
- `cargo fmt --all -- --check` passed.
- Isolated library tests for `ohagent-aggregator-core` and `ohagent-provider-metrics` passed: 19 tests, no failures. The aggregator crate currently has no library tests.
- Full parent runtime/E2E suites were not run. No ohAgent deployment, restart, production migration, credential change, or provider configuration change was performed.

Builds used a shared local Cargo target directory, which is removed after installation and completion of all validation. The pre-existing `TEAM_MEMORY/SESSION_LOG.md` working-tree edit is unrelated and excluded from this update.
