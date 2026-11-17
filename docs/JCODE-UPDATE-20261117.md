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

## Local installation outcome

The supported fast installer installed clean fork commit `1513367cd`. The launcher, current, stable, and shared-server channels resolve to that immutable binary. The local daemon completed a graceful exec handoff, and `/proc/<daemon-pid>/exe --version` independently confirmed the same commit.

The binary reports `jcode v1.0.70-dev (1513367cd)`, not a new upstream release: development metadata adds the 70 fork-side commits in `v1.0.0..HEAD` to the base patch number. The integrated stable upstream release remains v1.0.0.

All builds finished before cleanup. The 40 GB source `target` directory and inactive scratch fixtures created by validation were removed. Installed binaries, source checkouts, configuration, runtime data, and the unrelated user session-log edit were preserved.
