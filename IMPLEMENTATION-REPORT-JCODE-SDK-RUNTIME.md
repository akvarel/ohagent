# Jcode SDK Runtime Migration Implementation Report

## Scope

This change implements the approved ohAgent/Jcode boundary plan:

- ohAgent uses the public `jcode-sdk` runtime boundary for gateway sessions;
- Jcode remains the agent engine and runs as an external tenant-private process;
- the private Jcode fork is reduced to generic SDK/runtime fixes based on upstream v0.76.0;
- container, Docker Compose, and Kubernetes packaging include the exact runtime binaries and persistent tenant layout;
- tenant isolation is explicit and tested through the public runtime path.

No production deployment, secret change, or protected-branch write was performed.

## Delivered architecture

### ohAgent

- Replaced gateway session execution through private Jcode application internals with `jcode_sdk::JcodeClient`.
- Added explicit `tenant_id` to session configuration and every session lifecycle lookup.
- Launches one private Jcode runtime per tenant domain with `inherit_logins = false`.
- Runs blocking SDK calls on `tokio::task::spawn_blocking`.
- Returns SDK assistant text to the gateway.
- Forwards text, image, model selection, interrupt, cancel, archive, detach, and removal through public SDK methods.
- Creates missing tenant workspaces with owner-only permissions before runtime launch.
- Uses deterministic opaque SHA-256-derived workspace and runtime keys.

### Tenant runtime layout

The tenant runtime key is a 96-bit SHA-256 prefix represented by 24 hexadecimal characters. Raw tenant identifiers are never used in filesystem paths.

```text
/home/jcode/.ohagent/j/<runtime-domain>/rt-<24-hex-characters>/
```

- Docker image root: `/home/jcode/.ohagent/j`
- Docker Compose domain: `/home/jcode/.ohagent/j/compose`
- Kubernetes domain: `/home/jcode/.ohagent/j/$(POD_UID)`

The full worst-case Kubernetes Unix socket path measured 104 bytes, below the 108-byte `sun_path` limit. The runtime root remains under the persistent `/home/jcode/.ohagent` volume.

### Packaged binaries

The image builds and installs matching binaries from the pinned Jcode submodule:

- `/usr/local/bin/ohagent-daemon`
- `/usr/local/bin/jcode`
- `/usr/local/bin/jcode-harness-api-bridge`

Docker Compose parsing was added to the packaging acceptance contract. That check exposed a pre-existing malformed embedded Vault seed script, which was repaired.

### Minimal Jcode fork

The branch is based directly on upstream Jcode v0.76.0 and retains only generic changes needed by this boundary:

1. reconnect a pooled MCP server after its process dies;
2. report the requested working directory when attaching through the harness API;
3. tests freezing working-directory and unsupported permission-response behavior;
4. fork-compatible workflow handling for an absent optional `DEPLOY_KEY` and repositories with Issues disabled, without bypassing build, test, formatting, quality, or release gates.

Scheduler, TEAM_MEMORY, ambient-product, UI, and unrelated product customizations are not part of the minimal ohAgent runtime fork.

## Requirement-to-check traceability

| Requirement or changed public output | Concrete check | Observed result |
|---|---|---|
| Gateway must not depend on private Jcode application internals | Source scan under `crates/**/*.rs` for `jcode_app_core`; boundary compilation tests | 0 references in the ohAgent crates; public SDK boundary tests compile and pass. |
| Public SDK must create and run sessions | Credential-gated `deepseek_integration_test` using built `jcode` and `jcode-harness-api-bridge` | Real turn returned the expected assistant text through the SDK path. |
| Tenant ID must be explicit | `session_config_requires_explicit_tenant_id_and_rejects_private_internals` | PASS; blank tenant IDs are rejected. |
| Raw tenant identity must not enter paths | `runtime_path_is_hashed_and_does_not_expose_raw_tenant_id` | PASS; unsafe/raw tenant text is absent. |
| Runtime keys must be deterministic | `runtime_path_uses_frozen_sha256_tenant_key` | PASS with frozen 96-bit vector `rt-80a707af7dc77ee1228f9127`. |
| Same tenant reuses a domain, different tenants are isolated | `same_tenant_reuses_runtime_path_and_different_tenants_are_isolated` | PASS. |
| Cross-tenant session access must be hidden/rejected | Live DeepSeek test creates under tenant A, then performs tenant B lookup/list/archive attempts | Same-tenant access succeeds; other tenant sees no session and archive is rejected. |
| Runtime path must fit Unix sockets | `runtime_path_rejects_a_root_that_cannot_fit_the_unix_socket` plus explicit worst-case measurement | PASS; overlong root rejected; packaged Kubernetes layout measured 104 bytes. |
| Runtime state must be persistent in packaged deployments | `scripts/test-jcode-sdk-packaging.sh` literal contract and image inspection | Docker, Compose, and Kubernetes roots are below `/home/jcode/.ohagent`; container root exists and is writable by the non-root user. |
| Missing workspaces must not cause misleading binary errors | Live test starts with a nonexistent workspace; focused workspace validation tests | PASS; workspace is created and the real turn completes. |
| Image input must use the SDK path | Gateway dispatch tests and `send_message_with_images` boundary tests/source path | PASS; gateway calls SDK-backed image method and assistant response is preserved. |
| Interrupt and cancel must use SDK lifecycle calls | Boundary tests and source verification for `soft_interrupt`/`cancel` | PASS. |
| Gateway must return assistant text | Gateway dispatch tests and live DeepSeek assertion | PASS; exact assistant text reaches the caller. |
| SDK permission behavior must fail closed | `permission_response_is_rejected_when_bridge_has_no_permission_capability` | PASS; no false permission capability is advertised. |
| Dead MCP process must be replaced | `mcp::pool::tests::begin_connect_replaces_dead_client` | PASS. |
| Attached harness session must report working directory | `attached_session_reports_requested_working_dir` | PASS. |
| Fork CI must run without unavailable optional repository features | `scripts/test_fork_ci_workflow.py`, `cargo fmt --all -- --check`, and GitHub PR checks | Local contract tests pass; optional SSH setup is gated; disabled repository Issues no longer make the linked-issue check impossible; required build and quality jobs remain enabled. |
| Container must contain matching runtime executables | Docker build plus non-root container smoke test | PASS; all three binaries found, `jcode --version` reports v0.76.0-dev, runtime root is writable. |
| Docker Compose public path must parse | `docker compose config --quiet` inside packaging contract | PASS after repairing the seed-script indentation. |
| Kubernetes manifest must remain parseable | PyYAML parse of `k8s/base/deployment.yaml` | PASS. |

## Validation executed

### Passed

- `cargo test -p ohagent-core --test jcode_sdk_runtime_boundary_test`: 7 passed.
- `cargo test -p ohagent-core --test deepseek_integration_test -- --ignored --nocapture`: real DeepSeek public-SDK acceptance passed after building the runtime binaries.
- `cargo test -p ohagent-gateway dispatch::tests`: 2 passed.
- `cargo check -p ohagent-gateway`: passed.
- `cargo test -p jcode-base mcp::pool::tests::begin_connect_replaces_dead_client`: passed.
- `cargo test -p jcode-sdk -p jcode-harness-api-server`: harness 66 passed; SDK suites 10, 10, 5, and 4 passed; doctest passed.
- `python3 -m unittest -v scripts/test_fork_ci_workflow.py`: 2 passed, covering optional SSH setup and disabled-Issues policy.
- `cargo fmt --all -- --check` in the minimal Jcode branch: passed.
- `bash scripts/test-jcode-sdk-packaging.sh`: passed, including `docker compose config --quiet`.
- Final Docker image build: passed, image `ohagent:jcode-sdk-runtime`.
- Container public contract: non-root process, all three binaries present, correct environment, persistent runtime root writable, Jcode version executable.
- Kubernetes YAML parse: passed.
- Focused Rust formatting and changed-file whitespace checks: passed.

### Broad-suite limitations

- The complete ohAgent-only package run passed 197 unit/integration tests, then failed on two pre-existing `ohagent-provider-metrics` doctests. Those unchanged module docs contain prose/math diagrams in Rust code fences, so rustdoc attempts to compile them as Rust.
- The full parent workspace reached the upstream `jcode-base` suite and reported 1300 passed, 9 failed, and 1 ignored. Six failures require the external `sqlite3` executable. The remaining three are upstream baseline expectations for provider lifecycle normalization, test-harness path detection, and sponsor provenance. None are in files changed by this migration.
- A full minimal-Jcode workspace attempt exceeded the 600-second execution budget while compiling. The changed Jcode packages and exact retained behavior were validated independently and passed.
- Full-project `cargo fmt --all -- --check` and `clippy -D warnings` are not acceptance-clean because the repository has existing unrelated formatting and warning debt. Focused formatting and compilation of the changed path pass.
- `graphify update .` completed for both the parent and minimal Jcode repositories; optional SQL extraction remained unavailable because `tree_sitter_sql` is not installed.

## Known runtime limitations

- The in-memory ohAgent session mapping is not automatically reconstructed from persisted Jcode sessions after daemon restart. Persisted transcripts remain available for a future explicit recovery workflow.
- A tenant runtime child that exits causes the current request to fail. Automatic in-process child replacement remains future recovery work; daemon restart relaunches runtimes.
- The harness does not currently advertise an end-to-end permission prompt capability. Permission responses are rejected rather than silently auto-approved.

## Delivery safety

- No production deployment or infrastructure mutation.
- No secrets added, printed, or committed.
- No direct write to `main` or `master`.
- Parent and Jcode work are isolated on non-protected feature branches.
- Unrelated `TEAM_MEMORY/SESSION_LOG.md` changes were preserved and excluded from migration commits.
