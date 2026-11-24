# ORANGEHAT-AAAS-001 M1 public UI handoff

## Scope

This packet changes only `crates/ohagent-dashboard`, excluding `src/aaas`, which belongs to the product-app worker. No backend, submodule, production, credential, or secret changes were made by this packet. No commit or push was made because the coordinator owns integration.

## Delivered files

- `src/App.tsx`: public routes and preserved operator routes. `/agent-as-a-service/app/*` lazily loads the default export from `src/aaas/AaasApp.tsx`, with no props. An explicit unavailable-build view is used if the module is absent when Vite builds.
- `src/public/Home.tsx`: corporate landing, evidence-backed ecosystem, approach and contact sections.
- `src/public/Product.tsx`: Agent as a Service product page and clearly qualified runtime model.
- `src/public/Demo.tsx`: separate lazy chunk with a deterministic, local three-step sample and reset. No agent execution or API requests.
- `src/public/PublicLayout.tsx`: responsive navigation, language selection, skip link, architecture diagram, footer, unavailable and not-found views.
- `src/public/locale.tsx`: typed EN/LV/RU dictionaries, persisted selection, disabled-storage handling and shared locale events.
- `src/public/catalog.ts`: typed product references, relationship classifications, dated evidence and description-only scope.
- `src/public/Metadata.tsx`: localized title, description, canonical, Open Graph, Twitter, structured WebPage data, and noindex for all non-public routes.
- `src/public/public.css`: scoped graphite/orange design, 390px and 1440px layouts, visible keyboard focus and reduced-motion support. Existing operator styling is not replaced.
- `index.html`, `public/orangehat.svg`: meaningful initial metadata and local brand favicon.
- `tests/public-ui.cjs`, `package.json`, `package-lock.json`: repeatable Playwright acceptance checks and declared development dependency.

## Route and locale contracts

Public routes are `/`, `/agent-as-a-service`, and `/agent-as-a-service/demo`. The old dashboard is now at `/operator`. Existing `/progress`, `/skills`, `/skills/:id`, `/memory`, `/keys`, and `/usage` remain mapped to the existing components.

Locale storage key: `ohagent-language`. Values: `en`, `lv`, `ru`. Same-window event: `ohagent-language`, with an optional string locale in `CustomEvent.detail`. Changes in another tab are synchronized via the storage event. The page language and localized metadata follow the selection. The product module receives no props and can use the same contract independently.

The diagram and copy distinguish an agent definition, the ohAgent runtime, a workflow stage, and an actual execution instance. Standard Agent Specification conformance is explicitly not verified. Portability is described as a design goal, not certification.

## Public evidence reviewed read-only on 2026-11-24

- `https://bugzero.lv/` returned its public product page. Its title and product copy describe application intelligence for autonomous regression, semantic application scanning and maintainable regression suites. Performance figures, free-tier claims, pricing and production status were deliberately not copied.
- `https://api.github.com/repos/Graphify-Labs/graphify` returned the public repository metadata. The public README confirms local deterministic AST parsing and queryable code relationships. Catalog link: `https://github.com/Graphify-Labs/graphify`. No third-party affiliation or verified integration is implied.
- `https://github.com/akvarel/ohagent` returned HTTP 200. Runtime descriptions also use the repository's `AGENTS.md` and delivery evidence.
- A verified company email/telephone was not established. The contact section explicitly says a verified contact is unavailable and no enquiry is sent. No fake email or contact form exists.

## Validation actually executed

- `npm run build`: passed on 2026-11-24 at approximately 21:05 UTC. Vite produced separate `Demo` and `AaasApp` JS chunks. An earlier compilation failed in reserved `src/aaas/auth.ts` and `src/aaas/i18n.ts`. Those files were not edited by this packet. After observing the other worker correct the errors, the build passed.
- `npx tsc -b`: passed again against final source at approximately 21:10 UTC.
- `npm run test:public`: passed **41** Chromium scenarios at approximately 21:10 UTC. Covers EN/LV/RU, persistence after reload, 390/1440 widths without horizontal overflow, exactly one public h1, route metadata, noindex, source links, no fake mailto, keyboard skip/focus, reduced motion, sample steps/reset, locale custom events, blocked localStorage, preserved operator routes, app wildcard mounting, 404, and no public API requests. Network assertions prove the landing does not load demo/app modules.
- `git diff --check -- crates/ohagent-dashboard`: passed.
- Full-page desktop/mobile screenshots for home and product were inspected. Temporary screenshot artifacts are cleaned after inspection.

### Reproduce

From `crates/ohagent-dashboard`, use two terminals:

```sh
npm ci
# If Chromium is not already installed, install it explicitly for local testing.
npx playwright install chromium
npm run dev -- --host 127.0.0.1 --port 5184 --strictPort
```

```sh
npm run test:public
npm run build
```

`PUBLIC_UI_BASE_URL` changes the browser test origin. `PLAYWRIGHT_EXECUTABLE_PATH` can select an existing Chromium binary. The test never sends credentials or touches production. Operator API calls are deliberately answered with a local synthetic 503, so route checks do not certify real authenticated backend success.

## Acceptance boundaries

- This packet verifies client metadata. Server `X-Robots-Tag`, static fallback routing and authenticated product workflows belong to coordinator/backend acceptance. Read-only inspection found planned server noindex headers in the concurrently developed `web_site.rs`, but this packet did not execute that backend.
- Public HTML initially contains home metadata. Localized and route-specific metadata are applied by React. This is a client-rendered site, not server-rendered or prerendered multilingual HTML.
- Runtime availability, standards conformance, pricing, product ownership beyond supported evidence, and production readiness are not inferred from the demo or catalog.
- Existing operator components remain otherwise unchanged, including their existing internal copy. New public pages and the new operator navigation/notice are localized.
- The successful production build predates the final metadata wording/demo step-label refinement by a few minutes. Final source passed TypeScript and the full browser suite afterward. The coordinator should rebuild its final integrated artifact after all workers finish.
