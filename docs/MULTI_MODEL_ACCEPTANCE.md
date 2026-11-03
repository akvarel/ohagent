# Multi-model document acceptance, 2026-11-03

## Scope and interpretation

The requested pilot compares scanned, photographed and handwritten document inputs using three configurable model slots during the first two months. Administrators may disable slots or change model/version identifiers without rebuilding. Applications conservatively use comparison for every supported document, including PDFs, rather than attempting an unreliable handwriting detector. Comparison does not expire automatically or remove human review. Final invoice approval remains separate.

Three connection profiles are operator-provisioned. Browser settings cannot edit provider endpoints, credentials or secret references. Configurable model identifiers are not evidence that a vendor actually supports a selected identifier or document format. Live credentials and provider provisioning require separate approval.

## Independent executable acceptance

All new multi-model acceptance below uses three explicitly local OpenAI-compatible HTTP fixtures. These are not OCR models and establish no handwriting accuracy, cost or real-provider availability. The application, gateway, core binary, persistence and domain writes are real local implementations operating on synthetic documents and disposable databases.

| Requirement | Check and observed result |
|---|---|
| Three models participate by default | `scripts/document_comparison_smoke.py`: actual core binary made three distinct outbound HTTP calls, retained three successful results and configured model attribution. Passed. |
| Disagreement and failure are explicit | Same script: changed amount exposed `fields.total_amount`; partial failure retained successes with `complete=false`; all failures returned no candidate. Passed. |
| Settings are system-administrator only | Core service key rejected for settings. Accounting actual gateway rejected member, tenant admin, role-only and scope-only tokens, accepted signed system role plus system scope. Passed. |
| Disable and update without rebuild | Core and accounting public settings APIs changed the first model version and disabled the third. Next document made exactly two calls and retained the disabled slot. Passed. |
| Concurrent updates and durable settings | Core stale revision rejected, settings survived actual process restart. Passed. |
| Original evidence survives human correction | Accounting applied document retained original bytes and the original comparison snapshot after review and balanced posting. New policy did not rewrite previous job results. Passed. |
| Tenant isolation and no duplicate accounting effects | Accounting real PostgreSQL workflow rejected foreign source/transaction reads and foreign supplier mapping, rejected unreviewed apply, and repeated apply retained one balanced transaction with two entries totaling EUR 123.45 debit and credit. Passed. |
| Flatly public document and settings boundaries | Actual packaged application and isolated PDF renderer passed invoice-to-expense and meter-to-reading writes, retained originals/reviewer/model snapshots, rejected replay effects and foreign company reads. Platform admin settings accepted, manager denied, stale revision rejected, disabled slot received no call, partial comparison required explicit acknowledgment. Passed after fixing an observed empty-list serialization defect. |
| Localized browser reviewer and administrator settings | Actual Firefox workflows passed in both apps: ordinary member/manager has no editor, authorized system administrator changed a version and saved through real APIs, then actual file-input upload used the new version and showed the disabled third model. Flatly also exercised its explicit Analyze action and blocked apply until human acknowledgment. English, Russian and Latvian labels/warnings observed. |
| Durable two-calendar-month application pilot dates | Both application public workflows retained pilot dates across uploads; actual browsers displayed November 3, 2026 to January 3, 2027 with continuing human review warning. Accounting application tests cover December 31 to February 28. Core intentionally does not own tenant pilot timing. |
| Actual three-vendor recognition accuracy | Not run. Requires approved configured credentials, supported endpoints and a representative approved handwriting/scan/photo dataset. |

Commands run from the ohAgent repository:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/document_comparison_smoke.py \
  --binary "$DOCUMENT_API_BINARY"

PYTHONDONTWRITEBYTECODE=1 python3 scripts/accounting_document_smoke.py \
  --ohagent-binary "$DOCUMENT_API_BINARY" \
  --gateway-binary "$ACCOUNTING_GATEWAY_BINARY" \
  --accounting-repo "$ACCOUNTING_REPO" \
  --provider-base-url http://127.0.0.1:1 --model fixture \
  --key-env ARTIFICIAL_UNUSED --comparison-fixture \
  --container "$LABELLED_DISPOSABLE_POSTGRES_CONTAINER"
```

The accounting fixture verifies the disposable container label and loopback port before creating an isolated schema, then removes its schema and child processes. The optional bounded browser fixture exposes only synthetic test session tokens on localhost and must never be used with a real customer or production environment.

## Final validation and observed repairs

- Root reran the final Rust suite: 51 passed, one optional isolated PDF runtime test ignored by the ordinary suite. The core worker separately executed that runtime test successfully. Root strict Clippy, workspace formatting and actual daemon compilation passed. Existing unrelated daemon warnings were not suppressed.
- Flatly frontend complete gate passed 365 files / 3,105 tests, lint and production build. Backend full verification executed 3,984 application tests plus 19 mail-module tests with zero failures/errors and one application skip. Verification still failed only at the unchanged baseline coverage gate: 25 missed classes, maximum 12. The final configuration regression package passed 69 focused tests and produced a fresh executable JAR.
- Accounting full Go race tests and vet, 12 focused frontend tests, TypeScript checking and complete production build passed. Root independently exercised the real public gateway and disposable PostgreSQL, then the actual browser controls.
- Independent acceptance found a real Flatly public serialization defect: an empty `disagreements` list was absent. Explicit stable serialization and an actual HTTP regression test fixed it. Both complete domain workflows then passed.
- Independent acceptance also found that the documented fallback administrator environment did not resolve correctly in the packaged application. Binding the primary and fallback properties independently fixed it. Root reran the **whole final PDF invoice, meter, settings, revision, history and acknowledgment workflow twice**, once with only `HOUSEHOLD_DOCUMENTS_ADMIN_KEY` and once with only `OHAGENT_DOCUMENT_ADMIN_KEY`. Both passed without a restart during model updates.
- Browser fixture lifetimes were bounded. After successful observations, the root intentionally stopped the localhost servers, cleared synthetic browser sessions, removed its disposable PostgreSQL container and temporary generated fixture directories. Cancelled server tasks are intentional teardown, not failed functional checks.

The Flatly command supports `--backend-jar` for an explicitly selected packaged artifact and `--admin-key-env` to isolate either documented server credential binding. No browser receives these credentials.

## Publication

All implementation and acceptance changes were committed and pushed to non-protected task branches:

| Repository | Branch | Commits |
|---|---|---|
| ohAgent | `feature/document-workflow-20261103` | `7f7b19c` core, `0b2a7ec` independent acceptance |
| Flatly backend | `feature/ohagent-documents-20261103` | `c647c11b`, final configuration repair `76e314a4` |
| Flatly frontend | `feature/ohagent-documents-20261103` | `3435085f` |
| OHAccounting | `feature/ohagent-documents-20261103` | `c24663c`, evidence `ea275ab` |

The final Flatly backend remote SHA was independently compared with local HEAD and matched. Pull requests were not created: the configured Bitbucket integration failed to initialize. No credentials or remote settings were changed to bypass this limitation. Unrelated `TEAM_MEMORY/SESSION_LOG.md` and frontend `test-results/` were preserved and excluded from task commits.

## Remaining release boundaries

No production deployment, service restart, live secret provisioning, account privilege grants or invoice distribution has been authorized or performed. Previous live single-model extraction checks do not establish the new three-provider behavior. The previously reproduced Flatly overall coverage gate failure, 25 missed classes versus maximum 12 on the pre-feature baseline, must not be hidden or disabled.
