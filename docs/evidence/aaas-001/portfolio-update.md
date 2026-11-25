# Portfolio expansion acceptance, 2026-11-25

## User journeys and implementation

Source: owner request to add Celotleti.lv as a flight-booking reference and Flatly.lv, Accounting and Medicine Reminder as products. Existing ohAgent, BugZero and Graphify entries must remain.

Visitors can read seven distinct cards in English, Latvian and Russian on desktop and mobile. Celotleti.lv is a client reference, not an ohAgent integration claim. Public links go to the two owner-supplied domains. Accounting and Medicine Reminder are described using their registered project documentation, without inventing a website, bot username or clinical benefit. Private project locations never enter the public catalog.

## Test-first evidence

- RED: `PUBLIC_UI_BASE_URL=https://stage.orangehat.ai npm --prefix crates/ohagent-dashboard run test:public` failed at the intended card-count assertion, `3 !== 7`, before implementation. Checkpoint `07ac40d` on `feature/orangehat-aaas-001`.
- GREEN: after TypeScript/Vite build, the same browser test against the actual local compiled site-only daemon (`PUBLIC_UI_BASE_URL=http://127.0.0.1:9192`) passed all 41 route/viewport/locale scenarios. The owned local daemon was stopped afterward.

| Requirement | Executable check | Local result |
| --- | --- | --- |
| Four additions, three entries preserved | Seven-card count, headings and existing BugZero/Graphify links | PASS |
| Accurate, localized summaries | Four product description assertions in EN/LV/RU at 390px and 1440px | PASS |
| Celotleti and Flatly public references | Exact HTTPS href, new tab and noopener assertions | PASS |
| No invented Accounting or Medicine Reminder URL | No anchor in either card, readable no-website label | PASS |
| Distinct product marks | New cards do not reuse Graphify fallback | PASS |
| Responsive layout and route safety | Existing overflow, metadata, no-public-API, demo/reset, keyboard, storage and operator checks | PASS |

Coverage is expressed as user-journey checks. No new instrumentation percentage is claimed. External booking, payments, medicine scheduling, clinical correctness and the products' own authenticated workflows are outside this catalog-only change. Existing customer inference remains disabled.

## Deployment

Staging deployment and remote checks are pending the new immutable image. Production is outside scope. Final digest and remote results will be recorded here after rollout.
