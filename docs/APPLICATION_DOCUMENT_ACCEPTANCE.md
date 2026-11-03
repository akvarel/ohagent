# Independent application document acceptance

Date: 2026-11-03. Coordinator-owned observations, not a deployment claim.

## OHAccounting: actual cross-service workflow

Executed `scripts/accounting_document_smoke.py` with the built restricted ohAgent service, the actual Go gateway binary, live `deepseek-flash`, and a separate labelled localhost PostgreSQL 16 container. The harness created its own unique schema from the real accounting migrations and generated an artificial invoice image. No customer documents or production databases were used.

Observed PASS in 11.4 seconds: authenticated HTTP upload, live structured extraction, invoice classification, tenant-owned supplier/account candidate lookup, mandatory human review, and actual balanced journal posting for 123.45 EUR. Original bytes were retrieved and compared. A foreign tenant could not read the source/job/transaction or inject its supplier. Posting before review was rejected. Duplicate upload and repeated apply returned the original job/effect. A direct SQL assertion observed exactly two balanced entries, 123.45 debit and 123.45 credit, linked to the source job.

Independent checks also passed:

- In `OHAccounting/gateway`, `OH_DOCUMENT_TEST_DSN=<isolated-local-test-DSN> go test -race ./... -count=1`, using real PostgreSQL 16. Live provider tests are opt-in and not silently counted by this ordinary suite.
- `go build ./cmd/server` and `go vet ./...`.
- In `OHAccounting/services`, `cargo test -p invoicing`: 15 tests, including required human approval, stale revision/edit invalidation, tenant boundary and service-auth rejection.

## OHAccounting: actual browser workflow

Foxhound MCP drove a new isolated browser container at `http://127.0.0.1:3302/dashboard/documents`. The actual Next application used same-origin rewrites to the actual synthetic gateway, which called the actual ohAgent service and live model. No document endpoints were mocked.

A second invoice was generated in browser canvas and uploaded through the page's file input. Observed invoice number `TEST-BROWSER-002`, amount `75.50 EUR`, classification and editable extracted fields. Posting remained disabled before review. The model misread supplier registration `40000000000` as `4000000000`, a real observed extraction error even on this synthetic printed document. The reviewer corrected it, explicitly selected the tenant-owned supplier and debit/credit accounts, confirmed review, then posted through the UI. The job became `applied`, the real journal contained its transaction, and API assertions confirmed that both original model value and corrected reviewed value were preserved. This is evidence that correction works, not evidence of OCR accuracy.

Rendered English, Latvian (`Dokumenti`) and Russian (`Документы`) headings and posted status were checked. A screenshot of the actual completed page was inspected. The browser console error filter returned no captured errors. Temporary browser session values were removed. The local UI and backend fixture were intentionally stopped after acceptance, and the coordinator independently verified no remaining fixture listeners or schemas.

The frontend integration check surfaced and fixed two configuration issues: explicit-empty `NEXT_PUBLIC_API_URL` now enables same-origin requests, and server-owned `OHACCOUNTING_GATEWAY_URL` selects the rewrite destination. Existing defaults were retained. The synthetic bootstrap endpoint is loopback-only, short-lived and never exposes a provider key.

## Flatly

Implementation and independent acceptance remain in progress. Do not infer Flatly success from the accounting results. Coordinator review flagged final release fingerprint coverage of company/recipient/PDF inputs and unsupported WebP advertisement; the implementation owner added mutation tests and removed WebP.

The first packaged fresh-Flyway HTTP acceptance started and authenticated successfully, but actual `POST /api/document-jobs` returned 400: the INSERT omitted `id`, while migration V161 declared a non-defaulted primary key. This defect was not caught by the focused schema-generated integration tests. Root reported the actual SQL stack to the implementation owner and requires a migration-backed regression plus rerun. This is a failed check, not successful runtime acceptance.

## Review limitations and release boundaries

The independent core security reviewer produced `DOCUMENT_SECURITY_REVIEW.md`; its two confirmed pre-body authentication/admission findings were reproduced with failing raw-socket tests and fixed. A separate application review worker stalled after initial source reads and was stopped, so no completed application-security review is attributed to that worker. The independent application evidence here is coordinator execution separate from the implementation authors.

The outgoing accounting invoice store remains an existing in-memory implementation, and its send action changes status rather than delivering actual messages. Its approval gate was tested, not real delivery. Production deployment, real migrations, provider region/DPA approval, customer-document use, representative handwriting evaluation, measured cheapest-model selection, and two months of pilot observation are not established by these tests. Automatic review bypass remains disabled.
