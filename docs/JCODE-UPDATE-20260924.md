# Jcode update acceptance, 2026-09-24

- Fork branch: `update/jcode-v0.88.0-20260924`.
- Merge: `1f8b6ba0c94af96c08bea87ef4e1a511b832878a`, preserving both the
  previous custom fork and upstream `v0.88.0` as ancestors. Pushed to fork.
- Parent gitlink update: `ce0bc1d`, pushed on the same named task branch.
- Existing uncommitted session-log changes were preserved and excluded.
- Locked workspace/all-target compilation passed. Application-core regression:
  1440 passed, 13 ignored. Memory regression: 112 passed, 1 ignored.
- Focused MCP: 41 passed, including real stdio collision tests. Existing
  team-memory guard tests: 10 passed. Independent read-only review found no
  confirmed merge regression in those boundaries.
- Formatting and diff checks passed. Four static ratchets also fail on untouched
  upstream v0.88.0; their checks and baselines were not weakened. Compiler warnings
  remain. See the submodule's `docs/FORK-UPDATE-0.88.0.md` for details.
- Official fast-release installer succeeded in 256 seconds. Installed version:
  `jcode v0.88.56-dev (1f8b6ba0c)`.
- The running daemon executable was independently checked through `/proc` and
  reports this same version. Current, stable and shared-server channels resolve
  to the installed version. Graceful reload preserved this session.

## Figma MCP blocker

The official guide still specifies `https://mcp.figma.com/mcp`. The configured
`mcp-remote@0.14.3` is the latest published bridge version as checked against npm.
An MCP reload call stalled for 915 seconds and was interrupted. A separate retry
with an enforced 40-second deadline completed in 5 seconds with
`RegistrationRejectedError: Dynamic Client Registration rejected (HTTP 403)`.
This occurs before interactive sign-in. Authentication and Figma tool access are
not complete. No credentials were reset, fabricated or embedded, and unrelated
MCP configurations were left unchanged.
