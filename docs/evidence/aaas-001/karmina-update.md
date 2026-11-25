# Karmina portfolio addition, 2026-11-25

Owner-provided description: Karmina is a platform for animation and photorealistic filmmaking. Its AI team consists of specialized agents, each focused on its own niche, under the direction of the author and director. Human creative control remains explicit. This catalog description does not certify an ohAgent integration or deployed filmmaking capabilities.

The catalog preserves seven existing entries and adds Karmina as the eighth product, with English, Latvian and Russian summaries and target-user copy. No public URL was supplied, so no website or private repository link is invented. The ecosystem introduction now includes filmmaking.

## Acceptance

- RED against existing staging: the new eight-card browser assertion failed as expected, `7 !== 8`, before implementation.
- Browser assertions cover Karmina title, both filmmaking modes, author/director control, specialized agents and their individual niches in EN/LV/RU at 390px and 1440px. Existing products, safe links, route isolation, accessibility and overflow checks remain.
- TypeScript/Vite build and 41 browser scenarios passed against the actual local compiled daemon. The first local attempt omitted `OHAGENT_DASHBOARD_DIR` and timed out waiting for the page controls. Supplying the documented built asset directory resolved the test setup issue, with no application or test bypass.

## Staging delivery

- Source commit: `864beb6` on `feature/orangehat-aaas-001`.
- Image: `rg.pl-waw.scw.cloud/orangehat/ohagent:stage-864beb6-karmina-20261125`.
- Registry manifest: `sha256:6ff595f33617ed159470e94d1d0ff53a3fb107b11f14d33e7af31ccda304b1d3`.
- Imported/pinned manifest: `sha256:f30aaa6193f892ec73c7d8ccb292e0b6e3f499f1e417c68b152f2c15269bc441`.
- Existing smoke-gated build and credential-free preload on four eligible nodes passed. Only the staging deployment image changed.
- Actual staging rollout passed on November 25, 2026 at approximately 16:09 UTC. Pod `ohagent-site-6b8c668fb-lng4s`: 1/1 Running, zero restarts.
- Real trusted-HTTPS acceptance: 2 tests passed. Private inference remains disabled and staging remains noindex.
- Real staging browser acceptance: all 41 scenarios passed, including eight cards, specialist-agent niches and author/director control in EN/LV/RU at both widths.
- Screenshots: `karmina-20261125/`. Actual mobile home screenshot visually inspected: Karmina text wraps correctly without overflow and existing cards remain intact.
- Production, credentials, other project workloads, Jcode submodule and unrelated session notes are unchanged. Owned build temporary context, smoke container and local preview process were cleaned up.
