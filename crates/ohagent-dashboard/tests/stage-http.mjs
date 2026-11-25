import assert from 'node:assert/strict';
import test from 'node:test';
import dns from 'node:dns';
import { isIP } from 'node:net';

const base = process.env.STAGE_SITE_TEST_URL;
assert.ok(base, 'Set STAGE_SITE_TEST_URL to the deployed staging origin');
// Optional newly published DNS override. HTTPS hostname/certificate checks remain active.
if (process.env.STAGE_SITE_TEST_ADDRESS) {
  const address = process.env.STAGE_SITE_TEST_ADDRESS;
  assert.equal(isIP(address), 4);
  const lookup = dns.lookup;
  dns.lookup = (hostname, options, callback) => {
    if (hostname !== new URL(base).hostname) return lookup(hostname, options, callback);
    if (typeof options === 'function') return options(null, address, 4);
    return options?.all ? callback(null, [{ address, family: 4 }]) : callback(null, address, 4);
  };
}

test('staging HTTPS serves nested routes, blocks indexing and exposes no enabled pilot', async () => {
  assert.equal(new URL(base).protocol, 'https:');
  for (const path of ['/', '/agent-as-a-service', '/agent-as-a-service/demo', '/agent-as-a-service/app/processes']) {
    const response = await fetch(base + path);
    assert.equal(response.status, 200, path);
    assert.match(response.headers.get('content-type'), /text\/html/);
    assert.match(response.headers.get('x-robots-tag'), /noindex/);
    assert.match(await response.text(), /<div id="root"><\/div>/);
  }
  const config = await fetch(base + '/api/aaas/config');
  assert.equal(config.status, 200);
  const body = await config.json();
  assert.equal(body.pilot_available, false);
  assert.equal(body.auth, null);
  const robots = await fetch(base + '/robots.txt');
  assert.equal(robots.status, 200);
  assert.match(await robots.text(), /Disallow: \/\s*$/);
  assert.equal((await fetch(base + '/sitemap.xml')).status, 404);
});

test('staging denies anonymous customer access and missing APIs/assets are not SPA success', async () => {
  const privateResponse = await fetch(base + '/api/aaas/processes');
  assert.ok([401, 403, 503].includes(privateResponse.status));
  for (const path of ['/api/nonexistent', '/assets/nonexistent.js']) {
    const response = await fetch(base + path);
    assert.equal(response.status, 404, path);
    assert.doesNotMatch(await response.text(), /<div id="root">/);
  }
});
