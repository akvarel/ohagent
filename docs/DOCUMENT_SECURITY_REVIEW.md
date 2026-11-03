# Independent document boundary security review

Date: 2026-11-03. Review scope: `crates/ohagent-documents`, `crates/ohagent-document-api`, and the document-router merge in `crates/ohagent-daemon/src/api.rs`. Snapshot: branch `feature/document-workflow-20261103`, HEAD `2ff8189`, with concurrent root-owned changes present. Line references below describe the inspected snapshot, not future fixes.

## Verdict

No demonstrated cross-tenant data disclosure, upload-controlled credential destination, tool execution, or payment/action execution was found in this boundary. Two availability/admission-control gaps remain. The existing tests pass but do not establish authentication before body consumption in the standalone service, bounded admission before decoding, live provider compatibility, or extraction accuracy. This report is review evidence, not final root acceptance.

## Prioritized findings

### F1: Standalone authentication runs after body buffering, allowing unauthenticated resource consumption

**Priority:** P1 if the standalone listener is reachable by untrusted clients, otherwise P2 defense in depth. **Confidence:** independently reproduced.

**References:** `crates/ohagent-document-api/src/lib.rs:62-66,91-99`; `crates/ohagent-document-api/src/main.rs:14-17`.

The handler takes `body: axum::body::Bytes`. Axum resolves that body extractor before entering the handler and therefore before `authorized()`. The 15 MiB body limit bounds one body, not concurrent bodies. There is no ingress concurrency limit or body-read deadline in this router. An unauthenticated sender can force body consumption and hold incomplete requests before the service checks the key. The four provider permits do not cover this stage.

**Independent loopback probe:** Built the current standalone binary with provider configuration removed. Sent raw HTTP requests without credentials:

- Complete two-byte `{}` body: `401 Unauthorized`.
- Declared two-byte body with only `{` sent: no response after 400 ms. Sending the final `}` then produced `401 Unauthorized`.
- Declared body length 15 MiB + 1 with no body sent: no response during the same 400 ms observation window. This is not a claim that an actually delivered oversize body bypasses the limit.

This proves the standalone route waits for body input before rejecting missing authentication. It does not demonstrate a measured out-of-memory event or establish an indefinite wait from a 400 ms observation.

**Important scope qualification:** The merged daemon route has `auth::require_auth` outside the document router (`crates/ohagent-daemon/src/api.rs:141-145`; middleware implementation `crates/ohagent-daemon/src/auth.rs:75-119`). With enforcement enabled, it rejects unauthenticated requests before the handler body extractor. Therefore this is not a demonstrated unauthenticated bypass of the normal merged daemon deployment. The standalone default bind is loopback, which also limits exposure unless reconfigured or proxied.

**Recommended acceptance:** Put authentication in request-parts middleware inside the shared document router, before body consumption. Add raw-socket tests where missing/wrong credentials plus an incomplete large body yield prompt 401 without completing the body. Separately define bounded body-read time and admission behavior for authenticated clients. Keep the existing body-size limit.

### F2: Capacity rejection happens after JSON allocation and full base64 decoding

**Priority:** P2. **Confidence:** verified source-order issue; overload impact not benchmarked.

**References:** `crates/ohagent-document-api/src/lib.rs:109-124`; `crates/ohagent-documents/src/provider.rs:64-65,215-242`.

The handler deserializes up to a 15 MiB JSON body and decodes up to a 10 MiB input before `try_acquire()` rejects saturated capacity. With an available OpenAI-compatible provider, `provider.extract()` validates and decodes the same upload a second time. Thus the four-permit limit bounds provider requests only, not expensive input preparation or all upload memory. Saturated authenticated clients can repeatedly incur decode/allocation work before receiving 429, and one tenant can occupy all four provider slots because admission is global.

The trusted-backend contract reduces direct attacker access, but a customer-facing adapter can still funnel bursts into this service. This is not an authorization bypass or evidence of retained tenant data.

**Recommended acceptance:** Acquire bounded processing admission after header authentication/tenant validation and before parsing/decoding, and apply ingress controls to body buffering as well. Avoid duplicate full decode where feasible without bypassing the public library's validation contract. Test that exhausted capacity rejects before invoking expensive validation/provider work, using a near-limit body. Define whether per-tenant fairness belongs here or in adapters, then verify that contract under concurrent tenants.

## Capability and acceptance gaps, not confirmed exploits

### G1: Provider compatibility and PDF support remain unproven

**References:** `crates/ohagent-documents/src/provider.rs:70-110`; `crates/ohagent-documents/tests/client.rs` tests `real_client_sends_inline_pdf`, `json_object_mode_supplies_schema_in_prompt_and_validates_locally`; `crates/ohagent-documents/src/lib.rs:1-12`.

The PDF path sends an inline `type: file` attachment. There is no PDF rasterizer, page-count enforcement, image dimension limit, or independent PDF/image decoding. The tests use magic-prefixed fixture bytes that are not real decodable documents. They verify request shape and local rejection, not provider PDF understanding. Byte limits and magic checks are useful but are not full scanning, validation, or decompression-bomb protection.

JSON-object mode correctly changes output-format configuration and supplies the schema in the prompt, while keeping local validation. This does not establish that a selected provider/model accepts the image/PDF attachment or supplies vision capability. An unsupported provider returning an error and the API returning 502 is safe failure. Claiming that JSON-object mode makes an otherwise unsupported provider a vision/PDF model would be false. No real provider calls were made in this review.

**Acceptance:** An explicitly approved representative dataset and provider-specific image/PDF contract check are required before claiming those capabilities. Unsupported inputs may fail closed. If deterministic local decoding/page bounds are required, that is additional implementation, not a property of existing magic checks.

### G2: Stateless local handling does not establish upstream data-retention guarantees

**References:** `crates/ohagent-document-api/src/lib.rs:96,100-108,122-129`; `crates/ohagent-documents/src/provider.rs:74-114`; `docs/DOCUMENT_WORKFLOW.md` responsibility boundaries.

The reviewed path has no document database, file write, chat-memory insertion, or document-content logging. Tenant IDs are syntax-checked and otherwise unused because the boundary is stateless. The service key authorizes trusted backends to assert tenant context, not individual end users. No cross-tenant lookup exists here to exploit. Adapters must independently enforce tenant membership, job ownership, target matching and persistence isolation.

All document bytes are sent to the configured provider. These sources do not establish the provider's retention, training, residency or account policy. Do not translate "stateless extraction" into "documents are never retained anywhere." Provider policy/contract acceptance remains an operational/privacy requirement. Review did not inspect secrets or make retention-policy claims about a particular provider.

### G3: Valid candidates are not business-valid or fact-verified extractions

**References:** `crates/ohagent-documents/src/provider.rs:245-331`; `crates/ohagent-documents/src/lib.rs:8-12`.

Decimal strings are nonnegative, finite lexical decimals, bounded to 40 characters and 12 fractional digits. Dates are exact digit-formatted calendar dates. Currency is checked against an explicit allowlist. These are useful fail-closed guards, not currency-specific scale/range checks, invoice arithmetic, meter plausibility, supplier identity verification, or factual OCR verification.

There is no requirement that each populated field have evidence, and a classification with all fields null and no warnings/evidence is accepted. Independently confirmed this yields HTTP 200 using a local fixture provider. This can be a valid uncertain extraction, but must not be advertised as a completed invoice or sufficient authorization to apply an action. Quotes and page numbers are model-supplied and cannot be verified against actual pages here. Signed credits/refunds fail validation because amounts must be nonnegative. That is safe rejection under the current contract, not silent support for credit-note accounting.

**Acceptance:** Adapter validation must reject missing/ambiguous required data, enforce domain-specific amount/scale/date constraints, tenant-owned matching and human-review rules. Handwritten recognition and evidence accuracy require representative ground truth, not fabricated provider responses.

## Positive controls inspected

- Destination and credentials come only from server configuration. Request schema rejects unknown upload properties; remote URL uploads are not supported. Endpoints require HTTPS except explicit loopback HTTP, reject URL credentials/query/fragments, and disable redirects. No upload-controlled SSRF or credential-forwarding redirect was demonstrated.
- Input accepts only bounded base64 PNG/JPEG/PDF signature matches. Filenames reject path/URL separators and control characters. File names are metadata, not local paths.
- Provider response consumption has a 256 KiB cumulative streaming limit, declared-length check, request/connect timeouts and no automatic redirect. Provider error bodies are not returned to clients.
- Local extraction decoding rejects unknown fields and missing nullable-field properties on the real provider path. Tool/function-call responses, refusal, truncation, malformed JSON and multiple/empty choices fail closed. Model attribution is overwritten from server configuration.
- The provider request has no tools. Document text is explicitly treated as untrusted data. This prevents local tool execution in this component, not hallucination, malicious output text, or unsafe downstream rendering/actions. Outputs remain untrusted.

## Validation actually executed

1. `CARGO_TARGET_DIR=/home/sergey/.cache/ohagent-documents-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 cargo test -p ohagent-documents -p ohagent-document-api`: **passed**, 8 API tests and 14 document client tests, plus zero-test binary/library/doc-test targets. No full workspace build/test was attempted.
2. Same environment with `cargo build -p ohagent-document-api --bin ohagent-document-api`: **passed**.
3. Independent raw-socket tests against a temporary loopback-only standalone subprocess with provider configuration removed: results recorded in F1. Process terminated afterward.
4. Independent HTTP integration probes against a temporary loopback-only fixture provider in JSON-object mode: all-null candidate **200**, duplicate nullable field **422**, missing nullable property **422**, unknown top-level action property **422**. Both local servers were stopped afterward. No secrets or real providers were used.
5. Source inspection of daemon merge/auth, configuration, schemas and tests. Full daemon runtime integration, browser/adapters, upstream provider compatibility, operational retention, load/latency benchmarks and real OCR accuracy were **not** validated.

Only this report was written by the reviewer. Implementation, tests, submodule and TEAM_MEMORY files were not edited. Root owns fixes, commits and final independent acceptance.
