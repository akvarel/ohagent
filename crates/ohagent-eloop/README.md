# ohagent-eloop

The engineering loop inside ohAgent.

The current vertical slice implements:

- short goal → repository context
- deterministic task contract (`task.json`)
- human-readable task (`task.md`)
- contract critic rejecting invalid verification commands such as `auto`
- atomic run storage
- safety policy
- usage schema with global totals and per-provider/model rows
- stable traits for the future embedded Jcode executor and independent verifier

## Build and test

```bash
cargo test -p ohagent-eloop
cargo clippy -p ohagent-eloop --all-targets -- -D warnings
```

The repository archive must contain the Jcode submodule before the complete ohAgent workspace can resolve its existing Jcode path dependencies.

## Create a plan

```bash
cargo run -p ohagent-eloop --bin eloop -- plan \
  "Prepare ohAgent for a safe Kubernetes startup" \
  --repository .
```

Artifacts are written under `engineering-runs/<timestamp>-<task-id>/`:

```text
goal.json
context.json
task.json
task.md
critic.json
token-usage.json
token-usage.md
```

## Next integration step

Implement a `JcodeTaskExecutor` using the existing `ohagent_core::JcodeBridge`:

1. create a headless session with the task repository as `working_dir`;
2. send `task.md` through the tool-enabled agent path;
3. capture structured execution events and full token usage;
4. run mandatory verification commands independently;
5. persist `result.json`, `verification.json`, `report.md`, and structured `next_tasks`.
