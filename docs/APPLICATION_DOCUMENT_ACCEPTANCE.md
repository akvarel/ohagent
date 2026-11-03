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

## Flatly: actual migrated API and live PDF workflow

Executed `scripts/flatly_document_smoke.py --pdf` against a fresh isolated file-backed H2 database created by the real Flyway migrations, the packaged Micronaut application, the actual restricted ohAgent binary, isolated Podman PDF rendering and live `deepseek-flash`. The existing deterministic acceptance identities and generated artificial documents were used. Real email, SMS, push and payments were disabled.

Both actual flows passed: PDF invoice `FLATLY-LIVE-001` for 123.45 EUR became a real property vendor expense, and PDF meter report `DOC-LIVE-MTR-001`, 160.00 on 2026-11-03 became a real reading on a tenant-owned synthetic meter created through the public domain API. Canonical vendor-invoice and meter-reading HTTP readback verified the amounts, identifiers, date and source linkage, not just job status. Originals matched byte for byte, authenticated reviewers were retained, duplicate upload and repeated application produced the same effects, and the seeded no-company staff identity received 403 for job/source reads.

The existing seed already has a 140 reading on this date. An attempted conflicting 160 reading on that meter was correctly refused with 409, and the job remained review-required. The harness then explicitly selected its separate newly created meter and observed successful application. No correction, chronology or domain validation was weakened to make the check pass.

Independent runtime testing first exposed two real defects that schema-generated/mock-provider tests missed: missing UUID defaults in the Flyway tables caused intake failure, and raw company names with spaces could not satisfy the core tenant-header contract. Migration V162 repaired both UUID defaults without rewriting V161. A spaced/Unicode company transport regression now verifies a stable backend-derived ASCII SHA-256 tenant token. Root reran the actual packaged workflow after both repairs and observed the passing results above. The review also required HTTPS except exact loopback, bounded whole-response timeouts and durable leases aligned with the shared service.

## Flatly: actual browser workflow

Foxhound drove the actual Vite application on loopback port 3303, with same-origin requests proxied to the synthetic Micronaut application and actual live extractor. The reviewer signed in through the existing login form, opened Internal Documents and Analyze documents, then uploaded a generated canvas PNG `artificial-browser-invoice.png`. It was retained before the explicit Analyze action. The model extracted invoice `BROWSER-FLATLY-002`, 57.25 EUR, supplier, date and a unique property suggestion, with visible warnings and evidence.

Apply was disabled both before allocation and before human confirmation. The reviewer explicitly chose the property and per-unit distribution, entered the expense name, and deliberately edited the synthetic supplier label to exercise reviewed-data preservation. After explicit confirmation, the UI applied the real expense. Independent browser API readback observed `APPLIED`, the authenticated reviewer, a real vendor invoice for 57.25 EUR, the original model supplier label still intact and the separately stored reviewed edit. This was an intentional fixture edit, not an observed OCR mistake on this Flatly document.

English, Latvian and Russian document-review/applied-state rendering were checked through the actual language switcher. A screenshot of the Russian applied review was inspected. The browser console error filter returned no captured errors. Synthetic logout returned 200, origin session/local storage was cleared, the fixture exited gracefully and independent socket inspection found no remaining fixture UI/backend listeners. The dev server was intentionally stopped after acceptance.

The final source packaged at 17:32:08 UTC was independently exercised again: actual PDF invoice and meter workflows both passed. Final unfiltered backend verification ran all 3993 reactor tests (3971 application tests plus 22 mail-module tests), with zero failures/errors and one skipped. The new document controller is covered. Overall Maven verification still fails the unchanged class-coverage gate, now 25 missed classes versus maximum 12, exactly matching the independently measured pre-feature debt. Final frontend gates passed 364 files / 3096 tests, lint and production build.

## Final exception-state browser acceptance

Against the final 17:32:08 UTC packaged backend and final frontend source, Foxhound opened the deliberately seeded `artificial-exhausted.pdf` job. EN/LV/RU showed the explicit manual-handling warning, retained source hash and original download, with neither an Analyze-job nor Apply action. An authenticated, company-scoped browser API read observed `EXCEPTION` and source download HTTP 200 with 631 original bytes. This fixture is deterministic UI evidence, not a claim that the live model failed three times. Integration tests independently cover the real three-attempt limit, expired third lease, unchanged original and no domain writes. Browser origin storage was cleared afterward.

## Independent pre-change coverage baseline

To establish attribution rather than assume that the coverage failure was pre-existing, root extracted commit `56525ee3cab12201afbe3766f3051fff48fe101a`, before this document feature, into a separate disposable source/build directory. The only diagnostic baseline change was the already-verified relative appointment date in `MaintenanceRequestTimelineTest`, preserving all assertions so its expired November 2 fixture did not mask the coverage result. No document implementation, tests, thresholds or exclusions were copied into the baseline. An independent build-directory lock prevented collisions, and the normal `mvn -o verify` lifecycle ran offline with a 600-second bound.

Observed result at 17:31:48 UTC: 332.5 seconds, 3972 tests across all reactor modules, zero failures/errors and one skipped. Verification failed specifically because **25 classes were missed versus the unchanged maximum 12**. Direct comparison of the actual baseline and task JaCoCo XML found the same 25 classes in both. The only additional missed task class at that checkpoint was `DocumentWorkflowController`; no baseline-only missed class existed. Final refinement covers that new controller, without exclusions or repairs to 25 unrelated components. Final JaCoCo XML confirms that only the same 25 pre-feature classes remain missed. This establishes an existing repository-wide coverage blocker, not a passing overall build. Compact evidence is retained here before removal of the disposable snapshot and generated logs.

## Review limitations and release boundaries

The independent core security reviewer produced `DOCUMENT_SECURITY_REVIEW.md`; its two confirmed pre-body authentication/admission findings were reproduced with failing raw-socket tests and fixed. A separate application review worker stalled after initial source reads and was stopped, so no completed application-security review is attributed to that worker. The independent application evidence here is coordinator execution separate from the implementation authors.

The outgoing accounting invoice store remains an existing in-memory implementation, and its send action changes status rather than delivering actual messages. Its approval gate was tested, not real delivery. Production deployment, real migrations, provider region/DPA approval, customer-document use, representative handwriting evaluation, measured cheapest-model selection, and two months of pilot observation are not established by these tests. Automatic review bypass remains disabled.
