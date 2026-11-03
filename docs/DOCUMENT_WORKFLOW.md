# Document workflow integration

Date: 2026-11-03. Status: implementation in progress, not deployed.

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
| Image/PDF intake and type/size validation | Shared-core tests and HTTP tests | Pending |
| Real vision request and strict structured extraction | Local HTTP provider contract tests, then permitted live provider smoke | Pending |
| No tenant crossover | Negative app authorization and matching tests | Pending |
| Classification and unique target matching | App adapter tests and seeded real API workflow | Pending |
| Review before application, no duplicate effects | Persistent-state and retry tests | Pending |
| Final invoice approval blocks unapproved sends | Existing send-path integration tests | Pending |
| Web upload and review | Browser workflow | Pending |

Real handwritten extraction accuracy is not established by fixtures or mock provider responses. Model selection requires a representative approved dataset, known-correct annotations, actual usage/cost, and explicit quality thresholds. No price or model ranking from earlier conversation is treated as verified implementation evidence.
