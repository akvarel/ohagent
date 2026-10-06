# ohagent-eloop implementation status

## Implemented in this archive

- Added `crates/ohagent-eloop` to the workspace.
- Added the standalone `eloop` binary with `plan` and `context` commands.
- Added repository context collection for instructions, file tree, build systems, verification commands, and Git state.
- Added versioned `CompiledTask`, safety policy, acceptance criteria, and verification command contracts.
- Added a deterministic bootstrap task compiler.
- Added a contract critic that rejects the `auto` verification placeholder.
- Added atomic filesystem run storage.
- Added global and per-provider/model usage structures and Markdown rendering.
- Added executor and verifier interfaces for the embedded Jcode integration.
- Added unit tests for context detection, compilation, invalid commands, atomic storage, and usage aggregation.
- Added system prompts for the future model-backed task compiler and critic.

## Not implemented yet

- Direct `JcodeBridge` execution.
- Streaming Jcode event capture.
- Provider/model/cached-token extraction from Jcode events.
- Independent execution of verification commands.
- Campaign state machine and automatic next-task continuation.
- Telegram command integration.

## Build limitation of the supplied archive

The uploaded source archive does not include the `jcode/` git submodule contents, while existing `ohagent-core` dependencies refer to paths inside that submodule. The execution environment also has no Rust toolchain. Therefore a real Cargo build could not be run here.

The new crate intentionally has no direct dependency on `ohagent-core` yet and can be tested independently. Before integrating `JcodeTaskExecutor`, restore the Jcode submodule and run:

```bash
cargo fmt --all -- --check
cargo clippy -p ohagent-eloop --all-targets -- -D warnings
cargo test -p ohagent-eloop
cargo build -p ohagent-eloop
```
