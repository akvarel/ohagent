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

`Extraction` retains `document_type`, typed nullable `fields`, `evidence`, `warnings`, `model`, and adds `meter_readings`, an optional/default-empty array when reading legacy responses. New provider schemas require the array explicitly. Supported input types remain PNG, JPEG and PDF. Existing request magic/type validation, schema validation, decimal/date/currency semantics and output collection/text limits apply to each model result separately. Model attribution is overwritten with the snapshotted operator-selected identifier, never trusted from the model's response.

- Enabled slots run concurrently, with at most three calls per comparison. Results retain registry order, not completion order.
- All three slots remain visible. Disabled slots have `extraction:null` and `error_code:null`, and are not called.
- `complete:true` means **all three configured slots succeeded**, even if they disagree. Disabled slots or failures make it false. It does not mean agreement, correctness or approval.
- `disagreements` contains lexicographically sorted exact differing `document_type`, `fields.<field_name>` and ordered meter-row paths among successful results. `meter_readings.length` reports different counts. `meter_readings.order` reports different row-ID sequences at equal length. `meter_readings[i]` reports an absent row, and `meter_readings[i].<field_name>` reports any differing scalar, including `row_id`, at the zero-based source position. Missing rows differ from present rows with null values. Null versus string and `"12.30"` versus `"12.3"` differ. Evidence, warning prose and model identifiers are not field disagreements. With fewer than two successes the list is empty, which must never be presented as unanimity.
- `extraction` is only the first successful result in configured order, an editable candidate. There is no majority vote, authority score, automatic application or silent single-model fallback. All failures produce HTTP 200 with `extraction:null`, three failed results and `complete:false`.
- Per-result `error_code` values are stable localization codes: `provider_failed`, `provider_unavailable`, `provider_timeout`, `invalid_extraction`, `response_too_large`. Provider response bodies, credential values and free-text errors are never forwarded.
- Each extraction's serialized size is bounded below 256 KiB, leaving room for the duplicate candidate and metadata. The final serialized response has a hard 1 MiB limit. A defensive overflow returns 502 `comparison_too_large` rather than sending an oversized response.

Missing or invalid boot profiles, unavailable/invalid persisted registry or unavailable registry storage fail comparison closed with HTTP 503 `model_registry_unavailable`, without using the legacy provider. All-disabled policy is allowed to persist, but comparison returns HTTP 503 `all_models_disabled` with zero provider calls. Invalid documents return 400 `invalid_document`. Total HTTP deadline expiry returns 408 `request_timeout` rather than falsely reporting a completed comparison. Existing structured HTTP errors retain `{error:{code,message_key}}`, with stable `documents.<code>` localization keys.

### Ordered meter-sheet observations (2026-11-04)

Only `document_type:"meter_reading"` may have a nonempty `meter_readings` array, with at most 100 rows. Each row has exactly these required properties, including explicit nulls:

```json
{
  "row_id":"observation-1",
  "unit_number":"1",
  "meter_number":null,
  "owner_name":null,
  "reading_value":"100.25",
  "reading_date":null,
  "reading_date_text":"30.09",
  "evidence":[{"field":"reading_date_text","page":1,"quote":"30.09"}],
  "warnings":["Year, meter identifier and owner are not visible."]
}
```

- `row_id` is a unique, nonempty, at most 64-byte ASCII identifier matching `[A-Za-z0-9_.-]+`. The other six scalar properties are nullable strings. Non-null strings must be nonblank and at most 2048 UTF-8 bytes, without disallowed control characters.
- A row represents **one observation**, not one apartment. Seven apartments with previous and current readings produce 14 rows in original source order, without sorting, merging, voting, deduplicating or choosing one reading. All original rows, evidence and warnings remain in each model's attributed result. The editable candidate is still only the first successful configured slot.
- `reading_value` is a plain nonnegative decimal string, at most 40 characters with at most 12 fractional digits, using existing scalar decimal semantics. No floating-point conversion or normalization occurs.
- `reading_date` is a real full ISO date `YYYY-MM-DD` or null. `reading_date_text` preserves the visible text, including `30.09` when no year is visible. A missing year must never be inferred from current time, filenames or metadata. A non-null normalized date paired with numeric day/month-only text (`30.09`, `30.09.`, `30/09` or `30-09`) is rejected locally. Unknown or uncertain row values remain null with row warnings. Any null scalar requires a nonempty warning array.
- Row evidence uses the existing exact `field`, `page`, `quote` shape, with only the six row scalar field names. Evidence must reference non-null fields. Quotes are nonblank and bounded to 2048 UTF-8 bytes, pages to 1..10000 and, for images/rasterized PDFs, the actual supplied page count. At most 64 evidence entries and 32 nonblank warnings of at most 2048 bytes are allowed per row. These limits do not independently verify quotes against pixels.
- With nonempty rows, scalar `meter_number`, `reading_value`, `reading_date` and `unit_number` must be null. A shared `property_address` remains scalar. Existing single-reading scalar responses and requests remain supported with an absent/empty array. `meter_readings:null`, unknown row keys, missing row properties, invalid dates/decimals, duplicate IDs, and over-limit arrays fail validation rather than being repaired or truncated.
- The prompt requests every observation. If a sheet visibly exceeds 100 observations it requests no rows and an explicit complete-sheet/manual-handling warning, never an apparently complete truncated prefix. Completeness and visible-year truth cannot be proven by structural validation and remain human-review responsibilities.
- Both JSON-schema and JSON-object modes allow at most 16384 completion tokens to accommodate multi-row output. HTTP/comparison/overall deadlines, no-retry/no-fallback behavior, 256 KiB provider-response limit, 1 MiB comparison limit, tenant isolation and credential handling are unchanged. A length-truncated response remains invalid output, not a partial success.

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
| Fourteen ordered observations, partial dates, shared address and model attribution | `transport_preserves_all_fourteen_rows_in_both_output_modes`, `row_candidates_keep_all_observations_and_first_success_not_majority` |
| Additive legacy array compatibility and exact row schema | `legacy_without_array_still_reads_and_keeps_scalar_reading`, `transport_accepts_legacy_scalar_responses_without_array`, `every_row_property_is_required_and_unknown_properties_are_rejected` |
| Bounded rows, unique IDs, null evidence, semantic dates/decimals | `rows_reject_invalid_ids_duplicates_overlimit_and_scalar_conflicts`, `rows_validate_decimals_dates_uncertainty_and_evidence_bounds`, `transport_rejects_incomplete_duplicate_overlimit_rows_and_invalid_row_pages` |
| Exact row count/order/all-scalar disagreements without mutation | `row_disagreements_cover_every_scalar_exactly_without_normalization`, `row_count_and_source_order_differences_are_not_hidden_or_reordered`, `row_evidence_warnings_are_preserved_without_becoming_scalar_disagreements` |
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

Multi-row extension validation on 2026-11-04: the synthetic 14-observation regression first failed because the old extraction shape rejected `meter_readings`. After implementation and additive empty-array updates to the API test fixtures, `cargo test -p ohagent-documents -p ohagent-document-api` passed 70 tests (23 provider/semantic tests, 23 comparison/registry tests, 19 API tests, four actual-binary HTTP tests and one PDF configuration test). The existing opt-in isolated PDF runtime test remained ignored, not newly verified. Strict focused clippy for both packages with `--all-targets -- -D warnings`, `cargo build -p ohagent-document-api`, workspace formatting and scoped diff checks passed. Checks used `CARGO_TARGET_DIR=/home/sergey/.cache/ohagent-documents-build`, `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0`. Synthetic fixtures contain no customer document content. Live multi-row application acceptance remains a separate coordinator-owned check, not an implication of these tests.
