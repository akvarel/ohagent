# Flatly document integration discovery

Read-only inspection on 2026-11-03. Both application repositories were on `feature/ohagent-documents-20261103`. Backend was clean. Frontend had only untracked `test-results/`, preserved. No tests, builds, runtime requests, secret operations, deployments, or commits performed. This report file is the coordinator-authorized sole write exception.

Backend paths below are relative to `/sharedssd/git/orangehat/accounting/household-backend`. Frontend paths are relative to sibling `household-frontend`.

## Existing upload and filing

`app/src/main/java/com/household/controller/FilingCabinetController.java`

- Line 177: `POST /api/filing-cabinet/upload`, multipart part `file`. Authenticated and `companiesFor(authentication)` must be nonempty. Staff OR resident with some company relation may upload. There is no target company parameter or tenant ownership record for the uploaded object.
- `uploadBytes`, line 212: maximum 20 MiB, extension allowlist and `UploadContentInspector.matches` content check. Types: pdf, jpg, jpeg, png, gif, doc, docx, xls, xlsx, ppt, pptx, txt.
- Calls `S3Service.uploadFile` under `documents/<UUID>/<sanitized-display-name>`. Returns `{url,fileName}`. Does not OCR, classify, create a document row, or rate-limit uploads.
- `POST /api/filing-cabinet` creates `FilingCabinet` mapped to table `document`. Drops client id, defaults createdAt, shared=false, text/fileName empty. Requires householdObject, validates the stored building belongs to target company, requires staff write to company, sets issuer to company. Company can default from first caller company, which the new integration should avoid by requiring an explicit selected context.
- Attachment URL validation checks only storage resolution and `documents/` prefix. This is NOT proof the uploader or target company owns the attachment. Do not authorize analysis by trusting arbitrary `fileUrl`.
- List/categories scope companies and `DocumentVisibility`: staff see company documents, residents only shared documents. Download applies row visibility; unknown/foreign row produces same 403.

`app/src/main/java/com/household/model/FilingCabinet.java`: title required, maximum 200, text, category (60), shared, fileName, fileUrl, createdAt, status, issuer, version, householdObject, companyName, legal-hold fields. Status vocabulary is draft/issued/archived.

Frontend:

- `src/pages/Documents.jsx:178-240`: upload then create, caches successful upload for retry. No OCR/classification.
- `src/services/api.js:1376-1417`: filingCabinetService list/categories/create/remove/upload/download/lifecycle/addRevision/review/hold.
- `src/App.jsx:260`: staff `/documents`; line 320 owner `/owner/documents`.

Migrations in `app/src/main/resources/db/migration`: V37 file URL, V101 category, V135 revisions, V139 status/issuer/version/property, V159 legal hold.

## Existing document human review

`app/src/main/java/com/household/controller/DocumentLifecycleController.java`:

- Base `/api/filing-cabinet`, authenticated.
- Line 135: `POST /{id}/revisions` requires stored-company staff and `document:create`. Only draft allowed through `DocumentDraftPolicy`.
- `RevisionRequest` at line 102: reason, title, text, category, fileName, fileUrl, validUntil. First reason optional, later required. Snapshots frozen content, retains superseded revisions.
- Line 205: `POST /{id}/reviews` requires stored-company staff plus `document:review`, a current revision, and no prior review. Only `outcome=approved` is supported by `DocumentState.normalizeOutcomeOrRefuse`.
- Request has `{outcome,note}` but the implementation does NOT persist the note. There is no rejected or needs-correction review outcome.
- Reviewer and timestamp come from authenticated caller/server.
- Line 233: `GET /{id}/lifecycle` derives uploaded/approved/expired/superseded. This is distinct from document.status draft/issued/archived.
- Write missing/foreign document gives same 404, read missing/foreign gives same 403.

`DocumentDraftPolicy` prevents deleting approved/referenced/held records. It does not make approval a prerequisite for vendor expense creation or billing release.

`DocumentRevision` maps to `document_revision`. V135 revision.text is VARCHAR(255), not suitable for raw unlimited OCR text.

Frontend `src/components/DocumentLifecyclePanel.jsx` is the existing review UI. `src/pages/DocumentVerification.jsx` and public `/verify` are BILL SHARE-LINK VERIFICATION, not OCR or document review.

## Existing meter OCR and matching

This is the smallest already integrated extraction-to-human-confirmation seam.

Backend `app/src/main/java/com/household/metercapture/`:

- `controller/MeterCaptureController.java:55`: `POST /api/meter-capture/readings`, multipart `image`, optional expectedDigits, decimals, serialHint.
- `controller/MeterExtractionController.java:64`: `POST /api/meter-capture/extraction`, same input, candidate fields SERIAL/VALUE/UNIT vocabulary. No shipped extractor produces UNIT.
- `controller/MeterMatchingController.java:46`: `POST /api/meter-capture/matching`, `{candidates:[...]}`.
- Capture and extraction store NOTHING and always require confirmation.
- `service/MeterCaptureService.java:109-122`: any capability meters:read, meters:read-self, readings:create, readings:create-self. Caller-keyed rate limiter, 20 MiB image limit. This image validation is not identical to filing-cabinet content inspection.
- `service/PermittedMeterScope.java`: company meter access through staff list capability and caller companies, plus own apartment meters through ownership link. Matching never performs a global device search or trusts model tenant ids.

Provider seam:

- `spi/MeterReadingRecognizer.java`, auto-discovered `MeterReadingRecognizerRegistry` with @Singleton implementations.
- Config `household.meter-capture.recognizer`, default seven-segment, fallback manual.
- `provider/HttpOcrRecognizer.java`: config `household.meter-capture.http.url`, timeout-ms default 8000. POST request `{image:base64,contentType,expectedDigits?,decimals?,serialHint?}`. Response `{candidates:[{value,confidence,digits?,decimals?,x?,y?,width?,height?}]}`.
- This HTTP adapter has NO authentication-header support. Only use as-is over explicitly trusted private transport, or add an authenticated ohAgent recognizer adapter. No actual ohAgent endpoint was verified or invented.
- `provider/RegisterDigitsExtractor.java` wraps active recognizer output as VALUE candidates.
- `provider/BarcodeSerialExtractor.java` supplies serial from barcode/QR, not generic printed text OCR. A new `DevicePhotoExtractor` implementation is needed for ohAgent printed serial/unit extraction.

Frontend:

- `src/services/api.js:524-577`: capture/extraction/matching APIs.
- `src/components/MeterPhotoCapture.jsx`: fills row input only.
- `src/components/MeterPhotoBatchUpload.jsx`: fail-closed EXIF stripping, separate recognition/extraction/matching, photo beside identified device/previous value/read value/consumption, correction and explicit confirmation. Never posts a reading itself.
- `src/pages/Meters.jsx` and `src/pages/owner/OwnerMeters.jsx` own final save.

Final write:

- `app/src/main/java/com/household/controller/UtilityMeterController.java:871`: `POST /api/utility-meters/readings` accepts `ConsumptionReading`.
- Fields include meterId, readingValue, readingDate, source, provenanceExtractorId, provenanceExtractorVersion, provenanceConfidence, photoFingerprint.
- Strips client id, stamps receivedAt, checks stored meter -> building -> company staff, or owner of apartment plus readings:create-self (`requireWritableMeter`, line 1185).
- Preserves resident submission-window/finalized-period restrictions, sane date, correction links, same-date identity, backwards reading rejection.
- `ReadingProvenancePolicy` stamps confirmedBy from principal and deduplicates same device/value/photo in billing month. Provenance is caller-supplied, NOT a signed extraction attestation. Backend cannot prove a human clicked. Do not give the agent write-capable end-user credentials when human review is mandatory.
- Migrations V96 corrections, V97 source/review, V121 provenance, V136 receivedAt.

## Vendor invoice action

`app/src/main/java/com/household/controller/VendorInvoiceController.java`:

- `GET/POST /api/vendor-invoices`, `POST /api/vendor-invoices/preview`.
- Model VendorInvoice maps to `property_expenses`, NOT outgoing owner statement `bill`.
- Exact ExpenseRequest fields: companyName, expenseName, expenseType, documentNumber, supplier, documentDate (LocalDate), householdObject, distributionBasis, total (BigDecimal), comment, workOrderId, requestKey.
- Required company/name/property/supplier/date/distributionBasis/total > 0.
- Staff target company and stored property belonging to same company required.
- expenseType: UTILITIES, REPAIR, INSURANCE, MANAGEMENT, TAXES, OTHER.
- distributionBasis: PER_UNIT, PER_SQUARE_METER, PER_OCCUPANT, PER_CONSUMPTION, FIXED_PER_UNIT, CUSTOM_FORMULA. Must be explicit, not silently inferred from OCR.
- documentNumber unique per company, duplicate 409 (V79).
- requestKey <=100, company-scoped idempotency including insert-race recovery (V157).
- Expense date in released publication month refused 409.
- Optional workOrderId requires completed/closed work of same company/property.
- Preview computes real BillingService allocation without saving.
- IMPORTANT: create does NOT accept documentUrl or filingDocumentId, although legacy model/list exposes documentUrl. Source-document linkage needs an additive field/migration or relation. Do not claim the current POST attaches a source scan.
- V144 adds supplier/documentDate.

Frontend `src/pages/Expenses.jsx` manual form/preview and `src/services/api.js:660-674`.

## Real billing release and notification boundary

`app/src/main/java/com/household/controller/BillingRunController.java`:

- POST base and `/preview`: billing-run:prepare plus staff target company.
- Line 170 `POST /{id}/approve`: billing-run:approve plus staff of STORED run company, PREPARED -> APPROVED.
- Line 202 `POST /{id}/release`: billing-run:release plus staff stored company, APPROVED only.
- Calls `service/BillingReleaseService.release` at line 216. Transactional CAS release, refuses already-issued company/period, issues bills through BillingService.
- AFTER transaction returns, line 227 calls `service/BillIssuanceNotifier.notifyIssuedBills(company,period)` -> `NotificationService.notifyIssuedBill`.
- `BillDeliveryService` is a READ delivery projection, NOT dispatcher.

Critical limits:

1. `InvoiceScheduler` skips company/months WITH any billing run, but NO RUN still uses legacy unattended billing. Test `M04InvoiceSchedulerRunGateTest.aCompanyWithNoRunIsStillBilled` explicitly asserts that behavior. Thus current approval is not a universal issuance/dispatch gate.
2. No distinct-actor check appears in approve/release controller methods. Capability separation does not prove two different humans, especially platform admin.
3. Filing review does not gate expense booking or bills.
4. Nonproduction admin TestSendBillsController exposes `/api/test/send-bills` and `/send-single-bill`. Never give an agent those routes.

Proposal-only integration must not prepare/approve/release billing runs, call NotificationService/MailMediator, send invoices, change bank details, or make payments. A universal human-dispatch requirement needs explicit backend fail-closed policy at issuance/notification seams, including scheduler/no-run and alternate send paths, not UI-only gating.

## Recommended minimal implementation packet

### Lowest-risk existing seam

Add an authenticated ohAgent `MeterReadingRecognizer` plugin, keeping existing candidate response, scoped matching, correction, confirmation and manual final intake unchanged. Add a `DevicePhotoExtractor` only if serial extraction is required. Do not modify router provider selection with switches.

### Document-focused pilot

New backend `documentanalysis` SPI, registry, provider adapter, service, controller, DTOs. Frontend `Documents.jsx` candidate panel, `services/api.js`, localized strings. This is NEW capability, not existing generic OCR.

- Staff-only backend analysis request against an EXISTING authorized documentId/revision, or a separately content-validated multipart input.
- Resolve company from stored source and principal. Read S3 server-side, not arbitrary model/browser URL.
- Proposed response only: schemaVersion, requestId, sourceHash, providerVersion, suggestedAction=meter_reading|vendor_invoice|filing_document|unknown, candidates[{field,value,confidence,page?,boundingBox?}], warnings, requiresHumanReview=true.
- Suggestions confer no authorization and invoke no tools/actions.
- UI shows original with editable candidates, human selects action, then existing manual forms perform writes under human principal.
- Meter: SERIAL/VALUE advisory, scope-resolved target meter.
- Expense: supplier/documentNumber/documentDate/expenseName/total/expenseType advisory, human explicitly chooses property and allocation basis.
- Filing: title/text/category advisory, human chooses property, shared=false/status=draft, issuer remains server-derived.
- Unknown/unreadable/low confidence stays manual.

If durable async/human review is required, add ONE proposal/analysis table: company, source document/revision/hash, provider/schema version, bounded proposal, pending/approved/rejected state, human actor/time, idempotency key, resulting action/entity id. Atomic review CAS and idempotent execution. Reauthorize source/target and verify source revision/hash at acceptance. This state does not exist and must not be faked in document_revision.reviewOutcome. A synchronous advisory form-fill pilot needs no new DB state, but cannot claim durable review evidence. Linking accepted vendor invoice to cabinet source needs an additive relation/migration.

Security requirements: strict bounded schema, exact decimal money, no invented dates/amounts, prompt-injection untrusted input, no model tenant authority, no arbitrary URL fetching/SSRF, no general database/assistant tools, no secret in browser/output, no raw photos/OCR/PII in logs, size/page/deadline/user-company quota before paid analysis. Backend-only adapter over private or authenticated transport. Secret/access/egress changes require approval under this packet.

## Tests and Linux feasibility

No tests/builds executed. Environment checks succeeded: Maven 3.8.7, GraalVM Java 25.0.3, Node 24.20.0, npm 11.19.0. Frontend node_modules and backend app/target exist. cmd.exe and rg absent. Project release is 21, forked compiler configured in app/pom.xml:317-329. Compatibility/dependency completeness remains unproven.

`scripts/run-backend-suite.sh` provides flock serialization but defaults to Windows cmd.exe. It supports `SLM_SUITE_CMD` override. Candidate Linux command, NOT executed:

```bash
SLM_SUITE_CMD='mvn -o test' bash scripts/run-backend-suite.sh
```

Focused command should use the same wrapper lock with `mvn -o -pl app -am test -Dtest=<names> -Dsurefire.failIfNoSpecifiedTests=false`. Relevant tests:

- MeterReadingRecognizerContractTest
- MeterCaptureControllerTest
- MeterExtractionControllerTest
- MeterMatchingControllerTest
- VendorInvoiceControllerTest
- DocumentLifecycleAndRetentionTest
- ReadingProvenanceTest
- M04ReleaseIssuesBillsTest
- M04ReleaseNotificationsTest
- M04InvoiceSchedulerRunGateTest
- UploadContentInspectorTest

Normal full suite `mvn test` through wrapper, `mvn verify` for integration/coverage risk.

Frontend focused tests: documents-upload.test.jsx, document-lifecycle-panel.test.jsx, meter-photo-confirmation.test.jsx, meter-photo-extraction.test.jsx, expenses-page.test.jsx. Then `npm test` and `npm run lint`.

Real-stack `npm run test:cucumber` needs isolated seeded backend and frontend proxy. Update both USER_STORIES, deterministic seeder and acceptance scenarios for new workflow. No existing general document OCR acceptance workflow was found.

Root rules/current POM/source override stale routed docs that still claim Micronaut 4.9, insecure JWT defaults or CREATE schema. Current framework is Micronaut 5.1.0, JWT has no insecure default, Flyway/NONE schema defaults. No documentation repair performed in this packet.
