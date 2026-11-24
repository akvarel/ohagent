// Read-only live checks. No login, credentials, valid authorization codes or model calls.
import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'

const host = 'https://auth.bugzero.lv'
const targets = [['ohagent-stage', 'https://stage.orangehat.ai'], ['ohagent-prod', 'https://orangehat.ai']]
const results = []
for (const [clientId, origin] of targets) {
  const issuer = `${host}/application/o/${clientId}/`
  const metadataResponse = await fetch(issuer + '.well-known/openid-configuration', {
    headers: { Origin: origin }, redirect: 'error', signal: AbortSignal.timeout(15000),
  })
  assert.equal(metadataResponse.status, 200)
  const metadata = await metadataResponse.json()
  assert.equal(metadata.issuer, issuer)
  assert.equal(metadata.authorization_endpoint, host + '/application/o/authorize/')
  assert.equal(metadata.token_endpoint, host + '/application/o/token/')
  assert.equal(metadata.jwks_uri, issuer + 'jwks/')
  assert.ok(metadata.id_token_signing_alg_values_supported.includes('RS256'))
  assert.ok(metadata.code_challenge_methods_supported.includes('S256'))
  assert.deepEqual([...metadata.scopes_supported].sort(), ['openid', 'profile'])
  assert.ok(['*', origin].includes(metadataResponse.headers.get('access-control-allow-origin')))
  const keysResponse = await fetch(metadata.jwks_uri, {
    headers: { Origin: origin }, redirect: 'error', signal: AbortSignal.timeout(15000),
  })
  assert.equal(keysResponse.status, 200)
  assert.ok(['*', origin].includes(keysResponse.headers.get('access-control-allow-origin')))
  const keys = await keysResponse.json()
  assert.ok(keys.keys.some(key => key.kty === 'RSA' && key.alg === 'RS256' && key.use === 'sig'))
  assert.ok(keys.keys.every(key => !key.d && !key.p && !key.q && !key.k))
  const callback = origin + '/agent-as-a-service/app/callback'
  const authorize = new URL(metadata.authorization_endpoint)
  const params = { client_id: clientId, response_type: 'code', scope: 'openid profile', redirect_uri: callback,
    prompt: 'none', state: 'ohagent-read-only-check', nonce: 'ohagent-read-only-nonce', code_challenge: 'a'.repeat(43), code_challenge_method: 'S256' }
  authorize.search = new URLSearchParams(params).toString()
  const response = await fetch(authorize, { redirect: 'manual', signal: AbortSignal.timeout(15000) })
  assert.equal(response.status, 302)
  const location = new URL(response.headers.get('location'))
  assert.equal(location.origin + location.pathname, callback)
  assert.equal(location.searchParams.get('error'), 'login_required')
  assert.ok(!location.searchParams.has('code'))
  for (const foreign of ['https://untrusted.example/callback', targets.find(([id]) => id !== clientId)[1] + '/agent-as-a-service/app/callback', callback + '/extra']) {
    authorize.searchParams.set('redirect_uri', foreign)
    const rejected = await fetch(authorize, { redirect: 'manual', signal: AbortSignal.timeout(15000) })
    assert.equal(rejected.status, 400, 'unregistered and cross-environment callbacks must fail')
    assert.equal(rejected.headers.get('location'), null)
  }
  const tokenResponse = await fetch(metadata.token_endpoint, { method: 'POST', headers: {
    Origin: origin, 'Content-Type': 'application/x-www-form-urlencoded',
  }, body: new URLSearchParams({ client_id: clientId, grant_type: 'authorization_code', redirect_uri: callback,
    code: 'ohagent-read-only-invalid-code', code_verifier: 'a'.repeat(43) }), redirect: 'error', signal: AbortSignal.timeout(15000) })
  assert.equal(tokenResponse.status, 400)
  assert.equal(tokenResponse.headers.get('access-control-allow-origin'), origin)
  const tokenError = await tokenResponse.json()
  assert.equal(tokenError.error, 'invalid_grant', 'public client must reach code validation without client secret')
  assert.ok(!tokenError.access_token && !tokenError.id_token)
  results.push({ client_id: clientId, origin, issuer, callback, discovery: 200, jwks: 200,
    signing_algorithm: 'RS256', public_client_without_secret: true, browser_cors: true,
    anonymous_authorization: 'login_required', rejected_foreign_callbacks: 3,
    actual_user_login_executed: false })
}
const report = { executed_at: new Date().toISOString(), kind: 'live-public-oidc-registration-check-not-user-login', clients: results }
if (process.env.OHAGENT_OIDC_EVIDENCE) await writeFile(process.env.OHAGENT_OIDC_EVIDENCE, JSON.stringify(report, null, 2) + '\n')
console.log(JSON.stringify(report, null, 2))
