#!/usr/bin/env bash
# Local daemon + real discovery. No user login, inference or production writes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DAEMON="${OHAGENT_TEST_DAEMON:?Set OHAGENT_TEST_DAEMON to the compiled daemon binary}"
SCRATCH="$(mktemp -d "${JCODE_SCRATCH_DIR:-${TMPDIR:-/tmp}}/ohagent-oidc-smoke.XXXXXX")"
PID=""
cleanup() {
  if [[ -n "$PID" ]]; then kill -INT "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
  rm -rf -- "$SCRATCH"
}
trap cleanup EXIT
for ENVIRONMENT in stage prod; do
  (
    set -a
    source "$ROOT/config/aaas-$ENVIRONMENT.env.example"
    export OHAGENT_AAAS_ENABLED=1 OHAGENT_AAAS_DATA_ROOT="$SCRATCH/$ENVIRONMENT"
    export OHAGENT_SITE_BIND=127.0.0.1 OHAGENT_DASHBOARD_DIR="$ROOT/target/dashboard-dist"
    exec "$DAEMON" --site-only --health-port 9321
  ) > "$SCRATCH/$ENVIRONMENT.log" 2>&1 &
  PID=$!
  READY=0
  for _ in {1..60}; do
    if curl --silent --fail --max-time 1 http://127.0.0.1:9321/health > /dev/null; then READY=1; break; fi
    if ! kill -0 "$PID" 2>/dev/null; then cat "$SCRATCH/$ENVIRONMENT.log"; exit 1; fi
    sleep 0.5
  done
  [[ "$READY" == 1 ]] || { cat "$SCRATCH/$ENVIRONMENT.log"; exit 1; }
  CLIENT="ohagent-$ENVIRONMENT" node --input-type=module - <<'JS'
import assert from 'node:assert/strict'
const base = 'http://127.0.0.1:9321'
const response = await fetch(base + '/api/aaas/config')
assert.equal(response.status, 200)
const config = await response.json()
assert.equal(config.auth.client_id, process.env.CLIENT)
assert.equal(config.auth.issuer, `https://auth.bugzero.lv/application/o/${process.env.CLIENT}/`)
assert.equal(config.auth.token_endpoint, 'https://auth.bugzero.lv/application/o/token/')
assert.ok(!JSON.stringify(config).includes('client_secret'))
for (const headers of [{}, { Authorization: 'Bearer invalid-token' }, { 'X-Tenant-Id': 'attacker' }]) {
  const denied = await fetch(base + '/api/aaas/processes', { headers })
  assert.equal(denied.status, 401)
  assert.equal(denied.headers.get('cache-control'), 'no-store')
}
console.log(`PASS ${process.env.CLIENT}: actual daemon discovers registered issuer and denies unauthenticated access`)
JS
  kill -INT "$PID"
  wait "$PID"
  PID=""
done
