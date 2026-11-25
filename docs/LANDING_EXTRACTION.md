# Corporate landing extraction, 2026-11-25

The OrangeHat corporate website is maintained in `git@ssh.bitbucket.org:toposoft/landing.git`, canonical directory `~/git/orangehat/landing`. Its source is no longer part of the current ohAgent tree.

## Moved ownership

- `crates/ohagent-dashboard/src/public/` became landing `src/public/`: corporate home, portfolio/catalog, product introduction, labelled local demo, translations, styles and public metadata.
- Corporate SVG, robots/sitemap, public browser tests, staging Dockerfile/manifests/scripts and public-only screenshots/evidence moved with the website.
- The new landing builds independently using Node/npm, with no sibling checkout, Rust daemon, provider secrets or runtime API implementation.

## Preserved runtime

Operator pages and `src/aaas/` remain here. Root now opens the operator console. Agent workspace, OIDC callback, tenant-scoped APIs, durable event logic and provider execution are not moved or reconfigured. Workspace brand navigation remains within its own app. Marketing routes and the corporate sitemap are no longer served by the daemon. Runtime UI remains noindex and robots-disallowed.

The shell uses the existing runtime language mechanism rather than importing corporate locale/style code. New focused runtime browser acceptance preserves operator routes and workspace mounting in EN/LV/RU at mobile/desktop widths, and checks that the compiled assets contain no corporate product copy or demo chunks.

## Executed validation

- Landing: clean `npm ci`, TypeScript/Vite build, 3 real HTTP tests and 31 browser scenarios against its real standalone server passed.
- ohAgent: dashboard build and `cargo build --locked -p ohagent-daemon` passed. Existing compiler warnings remain.
- Actual compiled daemon with built dashboard: all 4 `tests/site-http.mjs` checks passed, including missing marketing routes, missing corporate sitemap, private headers and closed unauthenticated customer APIs.
- Runtime browser test: 42 route/locale/viewport checks passed. Operator API failures are explicitly intercepted for UI mounting only, not represented as authenticated runtime acceptance.
- All 11 existing client protocol tests passed.

## Deployment and history boundary

No staging/production rollout, DNS change, OIDC registration, credential change or new runtime hostname is included in this source extraction. Existing stage remains on the previous immutable image. Landing deployment ownership and cutover procedure now live in its repository. A chosen public `VITE_RUNTIME_ORIGIN` can link the corporate site to the separately hosted runtime, but no origin or auth callback is invented here.

Prior public Git commits still contain the original combined files. Removing them from this tree is not a history purge. No force push, visibility change or license change was performed. Existing runtime evidence and unrelated user session notes are preserved.
