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

Deployed at `https://stage.orangehat.ai/#ecosystem` on 2026-11-25 at approximately 15:31 UTC. Production is unchanged.

- Source/GREEN checkpoint: `bae968b` (RED checkpoint: `07ac40d`).
- Image tag: `rg.pl-waw.scw.cloud/orangehat/ohagent:stage-bae968b-portfolio-20261125`.
- Registry manifest: `sha256:8ac25acad6f7a8999ca6464aa862cb3306b405d795bac093aa82a4b2db9d0bc0`.
- Imported/pinned manifest: `sha256:0a32837b41981121c391dd6b7bb3efe7fba75c10aee1783e4dcb98f3e5cc026f`.
- `scripts/build-stage-image.sh`: locked daemon build, clean npm install, TypeScript/Vite build, real container health and disabled-config smoke all passed.
- Existing credential-free image preload passed on all four eligible K3s nodes. Only the staging deployment image changed; other applied resources were unchanged.
- Actual rollout passed, pod `1/1 Running`, zero restarts, TLS certificate Ready.
- `STAGE_SITE_TEST_URL=https://stage.orangehat.ai node --test crates/ohagent-dashboard/tests/stage-http.mjs`: **2 passed**, including trusted HTTPS, noindex and intentionally closed private operations.
- `PUBLIC_UI_BASE_URL=https://stage.orangehat.ai npm --prefix crates/ohagent-dashboard run test:public`: **41 passed**, including all seven cards at both widths and in all three languages. No DNS override or disabled TLS verification was needed in this follow-up.
- Actual staging screenshots in `portfolio-20261125/`; home screenshots at 390px and 1440px were visually inspected for product wording, wrapping and layout. Product-page screenshots retain regression evidence.

Build scratch directories and smoke containers were removed by the pipeline. The owned local preview daemon was stopped. Unrelated `TEAM_MEMORY/SESSION_LOG.md` and the Jcode submodule are preserved.
