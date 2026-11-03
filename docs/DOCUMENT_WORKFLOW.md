# Document workflow integration

Date: 2026-11-03. Status: final-source live local API and browser workflows passed in OHAccounting and Flatly, including manual exception handling. Existing Flatly repository-wide coverage debt blocks overall verification. Not deployed.

## Responsibility boundaries

The shared `ohagent-documents` library extracts and classifies untrusted documents using a configured vision API without coding tools. The daemon exposes `POST /api/document-processing/extract` to authenticated application backends. Application adapters derive tenant scope from their own authorization, retain original documents and durable jobs, match permitted targets, validate business rules, and apply through existing domain services.

The endpoint accepts `X-Tenant-Id` from the trusted backend, not directly from browsers. The existing daemon API service key authenticates this trust boundary. Do not expose this service credential to customers. Extraction is stateless and must not use shared chat memory or session logs.

## Wire contract

Request: `file_name`, `media_type`, `content_base64`. Response: `document_type`, typed `fields`, `evidence`, `warnings`, `model`. Supported document classifications: `invoice`, `receipt`, `meter_reading`, `other`.

Fields: optional `invoice_number`, `issue_date`, `currency`, `total_amount` (decimal string), `supplier_name`, `supplier_registration_number`, `meter_number`, `reading_value` (decimal string), `reading_date`, `property_address`, `unit_number`.

No extracted identifier is itself authorization. The application must independently resolve a unique tenant-owned target and reject ambiguous or unsupported actions.

## Review policy

For the first two months after pilot activation, a human checks documents and proposed actions before application. Turning off intermediate review requires an explicit policy decision based on measured results, not merely elapsed time. Final human approval before invoice distribution remains required. A content change invalidates previous approval. Provider configuration and credentials, production deployment, and sending real invoices require their separate established approvals.

## Requirement to evidence map

| Requirement | Acceptance path | Status |
|---|---|---|
| Image/PDF intake and type/size validation | Shared-core tests and HTTP tests | Passed, including isolated two-page PDF, malformed PDF and page-limit rejection |
| Real vision request and strict structured extraction | Local HTTP provider contract tests, then permitted live provider smoke | Passed: generated invoice and meter PNG/PDF through live DeepSeek and restricted HTTP API |
| No tenant crossover | Negative app authorization and matching tests | Both actual application workflows reject unauthorized job/source access; accounting also rejects foreign targets and journal reads |
| Classification and unique target matching | App adapter tests and seeded real API workflow | Actual invoices and Flatly meter PDF classified, tenant-owned candidates/readback checked |
| Review before application, no duplicate effects | Persistent-state and retry tests | Both real API and browser workflows passed, with original/reviewed separation and real domain effects |
| Final invoice approval blocks unapproved sends | Existing send-path integration tests | Accounting 15 invoicing tests passed; Flatly 3993 reactor tests passed (one skipped), unchanged overall coverage gate failed on the same 25 missed classes independently observed before the feature |
| Web upload and review | Browser workflow | Both actual UIs, live model, explicit review and real domain readback passed in EN/LV/RU |

## Runtime configuration and reproducible checks

The standalone `ohagent-document-api` binary binds loopback `127.0.0.1:9092` by default (`OHAGENT_DOCUMENT_BIND` overrides). `OHAGENT_API_KEY` must be at least 24 characters. The daemon route uses the daemon's existing service key. Authentication, trusted tenant header validation and four-request global admission happen before reading document bodies. An admitted request has a 100-second overall deadline. Application adapters should use a timeout of at least 105 seconds and their own per-tenant quotas.

Configure provider values through approved environment/Vault injection, never browser input: `OHAGENT_DOCUMENT_BASE_URL`, `OHAGENT_DOCUMENT_MODEL`, `OHAGENT_DOCUMENT_API_KEY`. No provider or credential fallback is attempted. Set `OHAGENT_DOCUMENT_JSON_MODE=json_object` for DeepSeek, otherwise strict `json_schema` is the default. Both paths validate the same structured output locally. Provider URLs require HTTPS except explicit loopback test endpoints, and redirects are disabled.

DeepSeek does not accept native PDFs on this integration path. For it set `OHAGENT_DOCUMENT_PDF_MODE=podman` and `OHAGENT_DOCUMENT_RENDER_IMAGE` to an already provisioned local image. This host acceptance used `docker.io/library/alpine:3.20`. Rendering requires Linux, `/usr/bin/podman`, host Poppler and fonts. No image is pulled or built automatically. Each PDF is parsed in ephemeral networkless, capability-free, read-only containers with CPU, memory, process, file-size and runtime limits. Only the operation's private output is writable. Provider credentials are not passed into containers. Input is limited to 10 MiB and ten pages, rendered images to 1600 pixels and 20 MiB total. Temporary document files are removed by RAII. Default `native` mode requires independently verified provider PDF support.

Executed on 2026-11-03:

- `cargo test -p ohagent-documents -p ohagent-document-api`: 14 provider tests, 10 actual HTTP API tests and one PDF configuration test passed. Two raw-socket incomplete-body tests first failed, then passed after early admission/authentication fixes.
- `cargo test -p ohagent-documents --test pdf_runtime -- --ignored`: actual isolated multipage render and malformed/excess-page rejection passed. This opt-in runtime check is intentionally not silently assumed by the ordinary suite.
- `cargo clippy -p ohagent-documents -p ohagent-document-api --all-targets -- -D warnings`: passed after fixing the middleware's needless borrow.
- `cargo check -p ohagent-daemon`: passed, including actual merged router integration. Existing unrelated crate warnings were not suppressed.
- `scripts/document_vision_smoke.py --binary <built-binary> --base-url https://api.deepseek.com --model deepseek-flash --key-env DEEPSEEK_API_KEY`, also repeated with `--pdf`: both runs passed exact invoice number/amount/currency and meter number/value/date checks. Only generated test documents were submitted. No real customer PII was used.

The binary is buildable independently of the full daemon. Full daemon compilation passed; application adapter/browser acceptance must be recorded separately, not inferred from core results. The independent review snapshot is in `DOCUMENT_SECURITY_REVIEW.md`; its F1/F2 findings were reproduced and fixed by pre-body middleware with bounded admission. Review concerns about provider retention/DPA, real handwriting accuracy, and business-level required-field validation remain distinct from core extraction correctness.

Real handwritten extraction accuracy is not established by fixtures or mock provider responses. Model selection requires a representative approved dataset, known-correct annotations, actual usage/cost, and explicit quality thresholds. No price or model ranking from earlier conversation is treated as verified implementation evidence.
