import test from 'node:test'
import assert from 'node:assert/strict'

// Run against the real local daemon --site-only, AaaS disabled. No intercepted
// requests, fixture auth, model calls, production writes or operator API keys.
const base = process.env.AAAS_SITE_TEST_URL || 'http://127.0.0.1:9190'
test('real same-origin server restores nested routes and protects private responses', async () => {
  for (const path of ['/', '/agent-as-a-service/app/runs/example', '/agent-as-a-service/app/callback', '/operator/progress']) {
    const response = await fetch(base + path)
    assert.equal(response.status, 200, path)
    assert.ok(response.headers.get('content-type').includes('text/html'), path)
    assert.equal(response.headers.get('x-content-type-options'), 'nosniff')
    assert.equal(response.headers.get('x-frame-options'), 'DENY')
    if (path === '/' || path.includes('/app/') || path.startsWith('/operator')) {
      assert.match(response.headers.get('x-robots-tag'), /noindex/)
      assert.equal(response.headers.get('cache-control'), 'no-store')
    }
  }
})
test('real missing API and assets never become successful SPA documents', async () => {
  for (const path of ['/api/nonexistent', '/assets/nonexistent.js', '/agent-as-a-service', '/agent-as-a-service/demo', '/api/aaas/runs/foreign/events?after=0']) {
    const response = await fetch(base + path)
    assert.notEqual(response.status, 200, path)
    assert.ok(!(await response.text()).includes('<div id="root">'), path)
  }
})
test('unconfigured customer API is explicitly closed without an auth bypass', async () => {
  const response = await fetch(base + '/api/aaas/config')
  assert.equal(response.status, 200)
  const config = await response.json()
  assert.equal(config.pilot_available, false)
  assert.equal(config.auth, null)
  for (const path of ['/api/aaas/processes', '/api/aaas/runs']) {
    const denied = await fetch(base + path, { headers: { 'X-Tenant-Id': 'attacker' } })
    assert.ok([401, 503].includes(denied.status))
    assert.equal(denied.headers.get('cache-control'), 'no-store')
  }
})
test('runtime is not indexed and no corporate sitemap remains', async () => {
  const robots = await fetch(base + '/robots.txt')
  assert.equal(robots.status, 200)
  assert.match(await robots.text(), /Disallow: \/\s*$/)
  assert.equal((await fetch(base + '/sitemap.xml')).status, 404)
})
