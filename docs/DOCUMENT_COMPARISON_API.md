# Document comparison and model registry API

Status: implemented locally on 2026-11-03. No production deployment, credential provisioning, live three-provider quality evaluation, or pilot activation is implied.

## Responsibility and pilot policy

For the first two months after pilot activation, applications must send every handwritten, scanned and photographed document through `POST /api/document-processing/compare`, including image-based PDFs. The same comparison path may be used for all supported documents. System administrators may explicitly disable configured slots. Applications retain original documents, each model result, the policy revision and human review, and must not automatically treat the first candidate or agreement as authority. The service does not infer handwriting from pixels or silently expire comparison after two months. Pilot activation dates, routing, review and policy transitions remain application/coordinator responsibilities.

`POST /api/document-processing/extract` retains its existing strict single-extraction request, response and provider configuration. It is not a fallback for comparison. A legacy extraction provider alone cannot enable comparison.

## Immutable boot configuration

`OHAGENT_DOCUMENT_PROFILES` is a JSON array of exactly three unique operator-owned connection profiles. Each profile has:

| Property | Meaning |
|---|---|
| `id` | Stable slot ID, 1 to 64 ASCII alphanumeric, `-`, `_` or `.` characters |
| `label` | Nonsecret operator label, nonblank and at most 128 UTF-8 bytes |
| `base_url` | Immutable OpenAI-compatible API root, with `/chat/completions` appended by the client |
| `api_key_env` | Immutable environment variable name holding the provider credential |
| `json_mode` | `json_schema` or `json_object`, both with identical strict local validation |
| `pdf_mode` | `native` for independently verified native PDF support, or `podman` for the existing isolated renderer |
| `render_image` | `null` or omitted in native mode, required pre-provisioned local image in podman mode |
| `model` | Initial nonsecret operator-selected model/version identifier, nonblank and at most 256 UTF-8 bytes |
| `enabled` | Initial boolean state, with no automatic fallback |

All properties except `render_image` are required, and unknown properties are rejected. No provider family, endpoint, model ID or version is hardcoded as a default. The transport is the existing OpenAI-compatible HTTP client, not a vendor-name switch or a new SDK. Operators must choose three genuinely independent intended models, verify endpoint capabilities, and arrange approved credentials separately. Different slot IDs do not themselves prove model independence or quality.

Illustrative structure only, replace every placeholder through approved configuration:

```json
[
  {"id":"slot-a","label":"Operator slot A","base_url":"https://provider-a.example.invalid/v1","api_key_env":"DOCUMENT_PROVIDER_A_KEY","json_mode":"json_schema","pdf_mode":"native","render_image":null,"model":"OPERATOR_SELECTED_MODEL_A","enabled":true},
  {"id":"slot-b","label":"Operator slot B","base_url":"https://provider-b.example.invalid/v1","api_key_env":"DOCUMENT_PROVIDER_B_KEY","json_mode":"json_object","pdf_mode":"native","render_image":null,"model":"OPERATOR_SELECTED_MODEL_B","enabled":true},
  {"id":"slot-c","label":"Operator slot C","base_url":"https://provider-c.example.invalid/v1","api_key_env":"DOCUMENT_PROVIDER_C_KEY","json_mode":"json_schema","pdf_mode":"native","render_image":null,"model":"OPERATOR_SELECTED_MODEL_C","enabled":true}
]
```

`OHAGENT_DOCUMENT_MODELS_FILE` is an operator-owned local registry path in an existing writable directory. The service initializes a missing file at revision 1 using the three boot profiles. It never replaces an invalid existing file with defaults. The registry persists across restart and is reloaded for each settings request and each new comparison. Changing boot defaults does not overwrite an existing public registry. Changing connection profiles or credential values requires an explicitly approved configuration/restart operation, not a registry PUT.

Credentials are resolved only from profile-designated environment variables at boot. They are never returned by these endpoints, serialized into the registry, passed to PDF containers or included in provider error responses. `base_url`, credential references, JSON/PDF mode and renderer settings are not part of the public registry. All profiles, including disabled ones, must have valid immutable connection settings. A missing, blank or malformed credential leaves an enabled slot visibly failed with `provider_unavailable`, while other slots continue and administrator settings remain accessible. A disabled slot requires no credential to be skipped. Credentials are captured at boot, so provisioning or rotation still requires a separately approved configuration/restart operation, not a PUT.

HTTPS is required except explicit localhost/loopback HTTP endpoints for local use. Redirects and URLs containing userinfo, queries or fragments are rejected. PDF rendering retains the existing networkless, read-only, capability-free, resource-bounded Podman behavior and never pulls images automatically.

## Authentication and admission

Comparison requires the existing service `Authorization: Bearer <service-key>` or `X-API-Key` plus a valid trusted `X-Tenant-Id`, exactly as legacy extraction does. Trusted backend adapters derive tenant scope from their own application authorization. The comparison library is stateless with respect to documents and tenants. It must not be used as shared chat memory or as authorization to apply extracted identifiers.

GET/PUT `/api/document-processing/models` require **only the separate administrator credential**, configured through optional `OHAGENT_DOCUMENT_ADMIN_KEY`. It must contain at least 24 UTF-8 bytes after trimming surrounding whitespace and differ from the service key. Missing, short or equal keys disable administrator access. A service key is never accepted for administration, and an administrator key is not accepted for document extraction/comparison. Registry settings are global system policy, not tenant-owned document data, so these administrator routes do not require `X-Tenant-Id`.

Browser code must never receive either service key or administrator key. Applications expose their own authenticated systemadmin-only backend proxies, with the administrator credential injected on the server. Environment names for application-side injection are application-specific, not a reason to provision real secrets without approval.

Authentication, comparison tenant validation and a four-request global nonqueueing semaphore happen before document/settings body buffering. Saturation returns 429. Model settings bodies are limited to 16 KiB, while document request bodies retain the existing 15 MiB transport limit and 10 MiB decoded input limit. The complete admitted HTTP request has a 100-second deadline, including body upload, snapshot loading, calls and response serialization. Each model call, including optional rendering, is bounded to 90 seconds and the underlying HTTP request to 60 seconds. Applications should retain an outer timeout of at least 105 seconds.

## Comparison wire contract

Request is exactly the legacy `ExtractRequest`, with no model, endpoint or credential selection:

```json
{"file_name":"scan.png","media_type":"image/png","content_base64":"..."}
```

Response is HTTP 200 for completed fan-out with any mixture of successes and failures:

```text
{
  extraction: Extraction | null,
  comparison: {
    policy_revision: integer,
    complete: boolean,
    results: [
      {
        id: string,
        label: string,
        model: string,
        status: "succeeded" | "failed" | "disabled",
        extraction: Extraction | null,
        error_code: string | null
      }
    ],
    disagreements: [string]
  }
}
```

`Extraction` is the unchanged strict legacy shape: `document_type`, typed nullable `fields`, `evidence`, `warnings`, `model`. Supported input types remain PNG, JPEG and PDF. Existing request magic/type validation, schema validation, decimal/date/currency semantics and output collection/text limits apply to each model result separately. Model attribution is overwritten with the snapshotted operator-selected identifier, never trusted from the model's response.

- Enabled slots run concurrently, with at most three calls per comparison. Results retain registry order, not completion order.
- All three slots remain visible. Disabled slots have `extraction:null` and `error_code:null`, and are not called.
- `complete:true` means **all three configured slots succeeded**, even if they disagree. Disabled slots or failures make it false. It does not mean agreement, correctness or approval.
- `disagreements` contains lexicographically sorted exact differing `document_type` and `fields.<field_name>` paths among successful results. Null versus string and `"12.30"` versus `"12.3"` differ. Evidence, warning prose and model identifiers are not field disagreements. With fewer than two successes the list is empty, which must never be presented as unanimity.
- `extraction` is only the first successful result in configured order, an editable candidate. There is no majority vote, authority score, automatic application or silent single-model fallback. All failures produce HTTP 200 with `extraction:null`, three failed results and `complete:false`.
- Per-result `error_code` values are stable localization codes: `provider_failed`, `provider_unavailable`, `provider_timeout`, `invalid_extraction`, `response_too_large`. Provider response bodies, credential values and free-text errors are never forwarded.
- Each extraction's serialized size is bounded below 256 KiB, leaving room for the duplicate candidate and metadata. The final serialized response has a hard 1 MiB limit. A defensive overflow returns 502 `comparison_too_large` rather than sending an oversized response.

Missing or invalid boot profiles, unavailable/invalid persisted registry or unavailable registry storage fail comparison closed with HTTP 503 `model_registry_unavailable`, without using the legacy provider. All-disabled policy is allowed to persist, but comparison returns HTTP 503 `all_models_disabled` with zero provider calls. Invalid documents return 400 `invalid_document`. Total HTTP deadline expiry returns 408 `request_timeout` rather than falsely reporting a completed comparison. Existing structured HTTP errors retain `{error:{code,message_key}}`, with stable `documents.<code>` localization keys.

## Mutable registry wire contract

GET returns only:

```json
{"revision":1,"models":[{"id":"slot-a","label":"Operator slot A","model":"OPERATOR_SELECTED_MODEL_A","enabled":true},{"id":"slot-b","label":"Operator slot B","model":"OPERATOR_SELECTED_MODEL_B","enabled":true},{"id":"slot-c","label":"Operator slot C","model":"OPERATOR_SELECTED_MODEL_C","enabled":true}]}
```

PUT accepts that same shape, carrying the last observed `revision`. It may change labels, model/version identifiers, ordering/candidate priority and enabled flags, including disabling all slots. Exactly the three configured unique IDs must remain. Endpoint, environment-reference, credential, renderer and arbitrary extra properties are rejected. A successful PUT increments the revision and returns the complete public registry. A stale revision or exhausted revision counter returns 409 `model_registry_conflict`. Invalid proposed registries return 400 `invalid_model_registry`.

Updates use a stable `<registry-path>.lock` sidecar with nonblocking OS locking shared between service instances, then write a private temporary sibling, fsync it, atomically replace the registry and fsync its parent. A conflicting active lock or unavailable local storage fails closed with 503, not an unbounded queue. Keep the directory operator-owned and do not remove/replace the sidecar while service instances run. No document content, tenant records, service/admin keys or provider connection properties are persisted. A storage error after replacement may mean a revision committed despite an error response, so clients should GET and reconcile rather than blindly replay.

Every comparison captures an immutable validated policy snapshot before provider fan-out. An in-flight request keeps its original revision, versions, enabled flags and result order when administrators update the registry. New requests use the reloaded revision without restarting. Operator filesystem changes must also be valid complete registries and be written atomically while honoring the sidecar lock. A malformed/oversized registry is not silently repaired or reduced to one slot.

## Requirement-to-check map

| Requirement | Executable check |
|---|---|
| Three explicit operator slots, no hardcoded default | `registry_requires_three_unique_operator_slots`, actual binary `missing_or_incomplete_profiles_fail_closed_without_registry_or_network` |
| Concurrent bounded fan-out | Barrier-based `three_calls_run_concurrently_and_preserve_server_attribution`, short-deadline `timeout_retains_successes_and_does_not_wait_for_summed_deadlines` |
| Exact disagreement and candidate-only behavior | `exact_type_and_null_string_field_differences_are_not_majority_authority` |
| Visible failed/disabled/all-failed results | `failures_and_disabled_slots_are_visible_without_false_unanimity`, `all_failed_invalid_and_panicking_providers_never_produce_a_candidate`, HTTP comparison tests |
| Strict local validation and output bounds | Existing adversarial provider tests, comparison invalid-result test and `comparison_result_size_bound_applies_after_json_escaping` |
| Admin-only settings, never service-key elevation | `service_key_cannot_administer_models_and_admin_key_cannot_extract`, `missing_or_identical_administrator_credentials_fail_closed`, incomplete-body HTTP test |
| No arbitrary connections, envrefs or secret persistence | `settings_reject_unknown_endpoint_and_secret_properties`, `update_cannot_add_remove_or_substitute_a_connection_profile`, persistence tests |
| Optimistic revisions and immutable in-flight snapshots | `snapshot_and_optimistic_update_isolate_inflight_policy_and_versions`, `active_fanout_keeps_snapshot_when_registry_changes_before_completion`, HTTP mutation/conflict tests |
| Atomic persistence, restart/reload, corrupt-file fail-closed | `atomic_file_persists_reloads_and_never_contains_connection_secrets`, `malformed_oversized_or_locked_file_fails_closed_without_replacing_it` |
| Real binary environment + actual HTTP transports + live outbound version change | `real_binary_uses_three_operator_profiles_and_mutates_only_public_registry` with three loopback HTTP servers and synthetic keys |
| Missing credentials do not prevent admin disable or other model results | `missing_credentials_remain_failed_or_disabled_slots_without_global_outage` with actual binary and HTTP providers |
| All-disabled policy sends no model requests | Library and HTTP all-disabled tests |
| Legacy compatibility and no fallback | Existing legacy HTTP tests and `missing_registry_never_falls_back_to_the_legacy_provider` |
| Tenant/pre-body admission preserved | Existing legacy tests and `comparison_and_admin_admission_reject_before_reading_incomplete_bodies` |

Run focused validation:

```sh
cargo test -p ohagent-documents -p ohagent-document-api
cargo clippy -p ohagent-documents -p ohagent-document-api --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p ohagent-daemon
```

Validation executed on 2026-11-03: 51 ordinary tests passed (19 API HTTP tests, four actual-binary environment/three-provider HTTP tests, 14 provider transport tests, 13 comparison/registry tests and one PDF configuration test). The opt-in isolated PDF runtime test was also explicitly run and passed multipage rendering and malformed/excess-page rejection. Strict focused clippy, workspace formatting check, standalone binary build and scoped diff checks passed. `cargo check -p ohagent-daemon` passed with pre-existing unrelated warnings. An initial test was RED before the registry types existed, then GREEN after implementation. Shared-target disk exhaustion during later linking was resolved, and final focused tests/builds used the separate home target without incremental/debug artifacts.

The isolated PDF runtime test remains opt-in and requires its documented local Podman image and Poppler prerequisites. These tests prove code paths and transport contracts, not live provider availability, handwriting quality, model independence, retention/DPA compliance, pricing or the two-month application's actual routing/review behavior. Three-provider live evaluation requires separately approved credentials and a representative approved dataset.
