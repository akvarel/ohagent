# AaaS pilot operations and release checklist

## Status and architecture decision

This branch implements a locally validated pilot, not a deployed or production-ready service. The site and customer app retain React/Vite. A small accessible SVG renderer is sufficient for the fixed four-role fork/join graph. Fetch-based SSE carries bearer authorization without query-string tokens. Snapshot and journal share the same SQLite transaction. ohAgent orchestrates, JcodeBridge/public SDK alone execute models. Definitions, durable tasks/attempts and actual Jcode sessions have separate IDs.

Customer routes live under `/agent-as-a-service/app`; legacy operator UI is preserved at `/operator` and the existing `/progress`, `/skills`, `/memory`, `/keys`, `/usage`. Public pages and customer chunks are split. API misses and missing assets do not receive successful SPA HTML. Private routes have noindex/no-store headers. This is client-rendered HTML, not multilingual SSR.

## Safe local public-site launch

From the repository root:

```sh
npm --prefix crates/ohagent-dashboard ci
npm --prefix crates/ohagent-dashboard run build
cargo build -p ohagent-daemon --locked
OHAGENT_AAAS_ENABLED=0 \
OHAGENT_DASHBOARD_DIR="$PWD/target/dashboard-dist" \
OHAGENT_SITE_BIND=127.0.0.1 \
/sharedssd/cache/ohagent-target/debug/ohagent-daemon --site-only --health-port 9190
```

The binary path above is this checkout's `.cargo/config.toml` target. On another machine resolve the Cargo target directory instead of copying this absolute path. `--site-only` does not load `keys.toml`, start messaging gateways or run scheduled work. Never use normal daemon startup merely to preview the website. With AaaS disabled `/api/aaas/config` explicitly returns unavailable auth/runtime and customer operations remain closed. Existing operator API is not included in site-only mode.

Test actual HTTP routing in another terminal:

```sh
cd crates/ohagent-dashboard
AAAS_SITE_TEST_URL=http://127.0.0.1:9190 node --test tests/site-http.mjs
```

## Customer pilot configuration

Configure only in an approved isolated environment using the existing credential mechanism. On November 24, 2026 the owner approved `stage.orangehat.ai` / `orangehat.ai` and explicitly authorized registration of `ohagent-stage` / `ohagent-prod` in the existing BugZero Authentik instance. Both public registrations now exist. No client secret was created or copied, and BugZero clients, production application workloads, DNS and certificates were not changed. See `AAAS_OIDC.md` for live verification and remaining deployment gates.

| Variable | Meaning |
| --- | --- |
| `OHAGENT_DASHBOARD_DIR` | Absolute built dashboard directory, not a development source tree |
| `OHAGENT_SITE_BIND` | Listen IP, loopback by default in site-only mode |
| `OHAGENT_AAAS_ENABLED=1` | Enable the dedicated text-only Jcode adapter |
| `OHAGENT_AAAS_DATA_ROOT` | Absolute persistent private directory for DB, tenant Jcode homes and run workspaces |
| `OHAGENT_AAAS_DATABASE_PATH` | Optional SQLite path, default `aaas.sqlite3` inside data root |
| `OHAGENT_AAAS_OIDC_ISSUER` | Existing approved HTTPS issuer, exact discovery issuer match |
| `OHAGENT_AAAS_OIDC_AUDIENCE` | Required API access-token audience |
| `OHAGENT_AAAS_OIDC_CLIENT_ID` | Approved public browser PKCE client ID, not a secret |
| `OHAGENT_AAAS_OIDC_REQUIRED_ACCESS_SCOPE` | Set to `openid` for these Authentik profiles; requires that access-token scope and `azp` matching client ID, rejecting ID-token substitution |
| Existing provider environment | Already-authorized DeepSeek service credential consumed by Jcode, never put it in browser configuration |

A complete OIDC configuration is required. Missing fields fail closed. Configured but unreachable/invalid discovery fails startup rather than bypassing authentication. Browser callback is `{approved-origin}/agent-as-a-service/app/callback`. Register the exact origin/callback through the identity owner's approved process. Browser requests `openid profile`, S256 PKCE and no client secret. The IdP must issue a signed JWT access token with the configured API audience by default, permit browser token/JWKS requests as needed, and issue RS256/ES256 ID tokens. The backend supports additional asymmetric access-token algorithms, but that does not expand browser ID-token support.

One workspace is derived from signed issuer + subject. This does not implement organizational membership administration or immediate membership revocation. SSE revalidates signature/expiry on every batch, not just connect. Local sign-out removes browser tokens but is not a global IdP logout/revocation guarantee. No production seed-tenant endpoint exists. For real acceptance use two separately authorized IdP accounts, not test fixture principals.

## Persistence and limits

First enabled startup creates additive `aaas_*` SQLite tables and uses WAL. There is no external migration command, no production database modification was performed. Schema/body contracts are pilot version 1. Use exactly one active daemon owner per DB. Startup reconciles unfinished runs to interrupted and fences previous producers. It does not resume or silently execute them again. Saving a process creates an immutable version, pinned by each run. Same start idempotency key and payload return the same run even after restart.

Hard admission bounds: eight running jobs, 64 SSE streams, 300 seconds/job, one model turn per role, four roles, no automatic retries, 256 retained process-version rows per principal / 4096 global, 100 runs per principal / 1000 global. Dedicated customer child runtimes are capped at 16 retained principals per process and have no idle eviction yet. Capacity returns HTTP 429. Text and output sizes are bounded. No monetary reservation or exact token metering is implemented in this adapter. Unknown usage is not shown as zero. Time limits are not a hard guarantee against a provider's residual in-flight billable work.

Materials, verified artifacts and Jcode transcripts are private persistent data. The data root is owner-only on Unix. Backups need the same access controls and approved retention. There is no deletion/pruning UI, automatic retention or migration to an existing external secret store. Do not place the data root inside the served static directory. Do not enable arbitrary tools, inherited logins, external integrations or file uploads for this profile.

Cancellation is requested durably first. The adapter submits SDK cancellation only after the specific turn's MessageAccepted barrier, waits for successful SDK turn completion, and joins both branches before confirming logical cancellation. Failure to confirm becomes interrupted/failed, not falsely cancelled. Independent SDK connections prevent parallel sessions detaching each other. Async cleanup releases local handles, not customer transcripts. SDK TurnDone is a logical boundary: physical provider termination after SDK abort timeouts is not certified. Do not advertise rollback of external effects or guaranteed zero residual cost.

## Acceptance commands

```sh
cargo test -p ohagent-aaas --locked
cargo test -p ohagent-core --test aaas_runtime_policy --test jcode_sdk_runtime_boundary_test
cargo test -p ohagent-core standard_agents::tests
cargo test -p ohagent-daemon cancel_ack_is_not_turn_termination
cargo check -p ohagent-daemon --locked
node --test crates/ohagent-dashboard/src/aaas/protocol.test.mjs
(cd agents/standardagent-analytical-brief && npm ci && node --test index.test.ts && node export.ts --check)
```

The opt-in actual Jcode/provider check uses private in-test identities and synthetic caller materials. It makes real inference calls using the existing authorized provider environment. It is not live OIDC/browser acceptance:

```sh
OHAGENT_LIVE_AAAS=1 \
OHAGENT_JCODE_BINARY=/absolute/path/to/installed/jcode \
OHAGENT_LIVE_EVIDENCE="$PWD/docs/evidence/aaas-001/live-runtime.json" \
cargo test -p ohagent-aaas live_two_principals_actual_jcode_parallel_brief -- --ignored --nocapture
```

The actual installed-runtime no-tools smoke does not make model calls:

```sh
OHAGENT_JCODE_BINARY=/absolute/path/to/installed/jcode \
cargo test -p ohagent-core live_text_only_sdk_has_no_tools_and_releases_session_handles -- --ignored --nocapture
```

With a local Vite server on 5184, browser checks are `npm run test:public` and `node tests/universe-replay.cjs` from the dashboard directory. Replay recording uses the saved actual-runtime trace, with explicit read-only fixture-identity labels. It is not a live authenticated customer recording. `node tests/universe-stress.cjs` owns a local server on 5185 and runs the ten-minute synthetic renderer profile. Its measurements exclude durable transport/backend lag and real concurrent model runs. Test HTML/TSX entries are not included in the production build.

## Approved staging release gate

1. Use approved staging origin `https://stage.orangehat.ai` and `config/aaas-stage.env.example`; the separate production origin is `https://orangehat.ai` with `config/aaas-prod.env.example`. Dedicated public clients have been registered and publicly checked. Do not reuse BugZero clients or deploy production merely because its identity registration exists. Real account login, runtime secret provisioning and staging deployment remain separate checks.
2. Confirm real company contact and legal/privacy publication data. Current page honestly says no verified contact is available. This does not satisfy the final contact requirement.
3. Build a pinned commit through the existing approved pipeline. This branch contains no automatic production deployment. Use an isolated data root/DB and one replica. Preserve existing gateways, billing and operator access.
4. Reverse proxy same-origin `/api/aaas` without buffering SSE, support idle timeouts above the ten-second heartbeat, forward Authorization, disable private caching, terminate approved TLS and keep source/test/data directories unserved. Validate nested route refresh, assets, callback and logout on that origin.
5. Have two real authorized accounts create different private tasks, run the pilot, inspect overlapping branches, refresh/disconnect mid-run, download artifacts and attempt cross-account URLs. Record 60–120 seconds of the actual authenticated workflow. Fixture recording is not sufficient.
6. Exercise cancellation and configured time/resource bounds against the selected runtime version. Confirm IdP revocation behavior and residual cancellation semantics before making stronger claims.
7. Measure staging runtime-to-durable and durable-to-UI p95, transport reconnect/reset counts and stale-owner rejections. Existing daemon Prometheus middleware currently covers the legacy API, not this separately merged router. AaaS-specific exported counters/histograms and billing-grade token/cost metering remain a release gap. Do not claim dashboards or alerting exist for these metrics.

## Rollback

No deployed service was changed by this task. For an approved future staging rollout: stop new admissions, request cancellation and wait for confirmed terminal states or the documented deadline. Stop only the isolated site process through its approved supervisor. Back up the entire AaaS data directory after the sole owner stops (or use SQLite's supported online backup), keeping WAL/SHM consistency. Never copy a live DB file alone.

Restore the previous pinned application artifact and its matching dashboard. Set `OHAGENT_AAAS_ENABLED=0` to close customer operations while preserving the public website. Preserve the DB, Jcode homes, artifacts and transcripts for authorized inspection. Do not delete tables, credentials or data to roll back. Older releases ignore additive tables, but compatibility with newer serialized run bodies is not guaranteed. Restore a matching backup only with explicit approval if necessary. Never run two versions concurrently against one SQLite file. Existing public/product routes, legacy operator URLs and event schema version 1 stay documented in `AAAS_CONTRACT.md`.
