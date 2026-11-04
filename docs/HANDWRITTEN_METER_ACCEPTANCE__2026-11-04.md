# Handwritten meter sheet: real-provider acceptance

Date: 2026-11-04. Scope: isolated local ohAgent service used by Flatly, not a deployment or domain-data import.

## User journey and evidence

The user supplied a handwritten JPEG containing seven apartment rows, each with previous and current readings. The year is absent. Only this explicitly supplied file was submitted to existing provider accounts. No credentials, source image, owner names, address, or raw provider responses are committed here. Private raw observations are retained outside Git under the operator's local state directory, with 0700 directory and 0600 result permissions.

Read-only model-list requests succeeded with existing DeepSeek, OpenAI and Google credentials. This establishes authorization to list models, not successful inference or available quota.

| Model | Actual outcome |
|---|---|
| `deepseek-flash` | Direct full-table transcription returned seven rows and 14 reading strings in 10.76 seconds. Final actual comparison API also accepted its strict extraction. |
| `gpt-6-luna` | Direct full-table transcription returned the same seven rows and all 14 exact reading strings in 9.74 seconds. Final rebuilt comparison API accepted its strict extraction. |
| `gemini-3.8-flash` | Direct inference returned HTTP 503, status `UNAVAILABLE`, explaining temporary high demand. Other actual comparison attempts retained the slot as failed. No successful Gemini extraction or quality claim is made. |

An independent local comparison asserted seven rows, exact equality of every previous/current reading string, and a null year from both successful models. The coordinator visually compared the readings with the image. This is one sample, not a representative handwriting benchmark, independent ground-truth annotation, or evidence that either model is cheapest. Token usage is retained privately, but monetary costs were not measured. Direct full-table transcription is a diagnostic request outside the application extraction schema and is NOT an end-to-end Flatly import.

## Demonstrated bug and bounded fix

Before the fix, GPT returned valid JSON with evidence quotes for `reading_value` and `reading_date` even though those singular fields were null because multiple readings and an absent year could not be represented. The existing semantic validator correctly refused that inconsistent output as `invalid_extraction`.

The system prompt now states the validator's existing rules: evidence may reference only populated fields, ambiguity belongs in warnings, no arbitrary sheet row may be chosen, and a missing year must not be inferred from current time, filename or metadata. The semantic validator, schemas, authorization, timeouts and human-review requirements are unchanged. Instructions reduce this observed failure but do not guarantee model compliance.

The final rebuilt service called the actual three configured HTTPS endpoints directly. DeepSeek and GPT both succeeded with null singular readings/dates and explicit multiple-row/year warnings. Gemini remained failed and `complete` remained false. Exact address formatting differed, and the API preserved that disagreement rather than asserting unanimity. No extracted readings were applied to any Flatly database.

## TDD and validation

Journey derived during this acceptance: an ambiguous multi-reading document remains a valid review candidate without invented scalar data or evidence against null fields.

- RED: `CARGO_TARGET_DIR=/home/sergey/.cache/ohagent-documents-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo test --offline -p ohagent-documents --test client prompt_preserves_ambiguous_multi_reading_documents_without_invalid_evidence` executed and failed on missing evidence prompt guidance. Checkpoint `7be5e87`.
- GREEN: the same command passed after the prompt change. The regression exercises both JSON Schema and JSON-object transports, checks null ambiguous values, empty evidence, server-owned model attribution, and all three guidance clauses. This is a transport/prompt contract check, not an OCR-quality assertion.
- `cargo test --offline -p ohagent-documents -p ohagent-document-api`: 52 passed, zero failed, one ignored opt-in PDF runtime test. No PDF path changed or PDF acceptance claimed in this JPEG-only task.
- `cargo clippy --offline -p ohagent-documents -p ohagent-document-api --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: initially found wrapping in the new fixture, then passed after correction.
- `cargo build --offline -p ohagent-document-api`: passed. The rebuilt binary was used for the final real-provider recheck.
- No new coverage measurement was performed. The change is prompt text with regression checks for both transport modes, not a claim of repository-wide coverage.

## Remaining acceptance blockers

1. Gemini must actually complete inference before three-model live acceptance can pass. A listed model and a valid key are insufficient. No fallback model was silently substituted, quota purchased or credential changed.
2. The existing application wire format has one `reading_value`, `reading_date` and `unit_number`, not an array. It cannot preserve this sheet's 14 values as structured application results. Full batch ingestion needs a versioned/additive multi-row contract, immutable per-model row evidence, row-level review and tenant-owned target resolution, explicit year selection and domain-safe application. No schema or UI support for that was introduced or implied here.
3. No production/development service was deployed or restarted. Existing provider settings were not changed. The local comparison registry and synthetic service key were temporary and removed on process exit.

Unrelated `TEAM_MEMORY/SESSION_LOG.md` edits were preserved and excluded from task commits and scoped diff checks.
