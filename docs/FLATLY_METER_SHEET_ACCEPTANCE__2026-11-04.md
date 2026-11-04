# Flatly ordered meter-sheet acceptance

Date: 2026-11-04. Local isolated acceptance only. No production deployment or credential changes.

## Implemented scope

The additive document contract retains up to 100 ordered observations, including owner text, apartment, exact decimal strings, original partial dates, per-row evidence and warnings. A seven-apartment previous/current sheet has fourteen observations, not seven combined readings. Missing years remain null. Every applied observation needs an explicit company-owned meter, full date, value and independent review. Source and per-model proposals remain immutable. Atomic application retains all ordered receipts and the legacy first result ID. A late conflict rolls back domain writes and the apply CAS.

## Independent checks

- Before implementation, the synthetic fourteen-row HTTP provider result was rejected by the old core. This established a real-stack RED baseline.
- With the implementation, `scripts/flatly_meter_sheet_smoke.py` passed against the actual restricted Rust API and packaged Micronaut service with fresh migrated file-H2. The three outbound model responses were explicitly synthetic local HTTP fixtures, not OCR accuracy evidence.
- The script checked source-byte retention, outsider denial, missing comparison acknowledgement, unauthorized target, incomplete rows, unreviewed rows, missing years and scalar bypass. Invalid decisions produced no writes.
- A conflict deliberately seeded on the last meter forced a real late domain failure. All earlier writes and the CAS rolled back. A separately retained corrected decision then produced fourteen ordered receipts. Canonical histories, exact values/dates, original comparison immutability and replay without duplicates passed.
- Actual Firefox/Foxhound UI acceptance used the real Vite frontend and that migrated backend. Initial fourteen date inputs were blank. All three model proposals plus candidate retained 56 visible source observations. Explicit meter/date selections and fourteen reviews enabled apply. Editing one value reset its row review and source confirmation and disabled apply.
- After renewed confirmation, actual UI application returned fourteen receipts. An authenticated canonical API read-back verified the corrected value while the original extraction retained the uncorrected value. English and Latvian labels were checked in the actual UI. Russian localization is covered by component tests, not claimed as a completed browser check.
- The browser-hold process later reached the runner's 600-second timeout after its successful acceptance output. This is a hold-session expiration, not a failed atomic-application assertion. The disposable browser session storage was cleared.
- Independent final Rust command: `CARGO_TARGET_DIR=/home/sergey/.cache/ohagent-documents-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo test -p ohagent-documents -p ohagent-document-api --quiet` passed. Worker validation also passed strict two-package clippy, workspace formatting and API build.

## Approved handwritten source

The supplied private JPEG was processed through the new core with existing operator credentials. DeepSeek and GPT each retained fourteen observations. All fourteen decimal values independently matched the visual transcription and neither model invented a year. Gemini returned `provider_failed`; this is not three-provider success or a universal handwriting guarantee. Names, address, source bytes, credentials and raw responses are excluded from Git. Private local results are not durable project fixtures.

The first real backend attempt exposed a comparison-envelope compatibility failure despite successful core results. Core emits the legitimate `meter_readings.order` disagreement when independently generated row identities differ. Backend's strict whitelist omitted that known path. The backend now accepts and independently recomputes this exact path, with a synthetic regression and no arbitrary-path relaxation. The corrected package passed 77 focused tests, and the unchanged private response parsed successfully. Independent complete synthetic real-stack acceptance passed again against the corrected JAR. At 14:42:20 UTC the approved actual JPEG also passed the complete upload, original download, outsider-denial, live three-slot comparison and persisted fourteen-row review-required path through the corrected backend. DeepSeek and GPT succeeded, Gemini failed visibly. Canonical retained candidate values all matched the fourteen visual readings and full dates remained null. No real-source application was performed because the year and meter targets require a human decision.

Final frontend validation passed 366 files / 3149 tests, lint with zero errors and four existing warnings, production build and the five-scenario/twenty-five-step Cucumber dry run. The coordinator independently reran all 44 meter-sheet component tests successfully. Decimal boundary tests reject overlong values without truncation or rounding.

## Additional coordinator validation at 14:47 UTC

To independently recheck the contract inspection, parsing/atomic application, review UI/locales and acceptance-contract work, the coordinator reran:

- Backend serialized `mvn -o -pl app -am test -Dtest=MeterSheetExtractionTest,DocumentExtractionTest,DocumentComparisonTest,DocumentWorkflowIntegrationTest,DocumentWorkflowFlywayTest -Dsurefire.failIfNoSpecifiedTests=false`: **36 passed**, zero failures/errors/skips, BUILD SUCCESS. Includes 23 actual workflow integration cases and the actual Flyway migration, plus strict row/comparison contracts.
- Frontend `npx vitest run src/test/meter-sheet-review.test.jsx src/test/document-workflow.test.jsx src/test/document-model-settings.test.jsx --maxWorkers=2`: **57 passed across three files**. Covers fourteen-row decisions, locales, decimal/date boundaries, source immutability, legacy review and model-settings authority.
- `npm run test:cucumber -- --dry-run --tags @OHAGENT-DOCS`: five scenarios and twenty-five steps resolved, zero undefined steps. They are explicitly skipped in dry-run and are not additional executed browser scenarios.

No implementation changes were needed. Backend remained clean and unrelated frontend `test-results/` was preserved. These focused successes do not override the full verify coverage limitation below.

## Gate limitations

Final-source unfiltered backend verify completed at 14:44:07 UTC: 3997 tests, zero failures/errors and one skip. Packaging succeeded, but overall verify failed the unchanged JaCoCo missed-class gate: 25 versus maximum 12. The earlier full run had the same gate failure. The corrected focused package passed 77 tests. No threshold or required check was weakened. Exact backend commands and regression details are recorded in `household-backend/docs/OHAGENT_METER_SHEETS.md`.

Bitbucket PR inspection through the configured `atlassian-bitbucket` MCP failed initialization twice with a closed connection. No authentication or provider configuration was changed to bypass that blocker. Branch pushes are separate from PR creation.
