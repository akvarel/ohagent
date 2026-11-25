import type { PublicConfig } from './types'
import { APP_ROOT, safeReturnRoute } from './protocol'
import { ClientError } from './errors'

type AuthConfig = NonNullable<PublicConfig['auth']>
interface Session { accessToken: string; expiresAt: number; issuer: string; clientId: string; subject: string }
interface Transaction { state: string; nonce: string; verifier: string; createdAt: number; returnTo: string; issuer: string; clientId: string }
const SESSION_KEY = 'ohagent-aaas-session'
const TRANSACTION_KEY = 'ohagent-aaas-oidc'
const base64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
const random = () => base64url(crypto.getRandomValues(new Uint8Array(32)))
export const redirectUri = () => `${location.origin}${APP_ROOT}/callback`
function store(key: string, value: unknown) { try { sessionStorage.setItem(key, JSON.stringify(value)) } catch { throw new ClientError('storage') } }
function remove(key: string) { try { sessionStorage.removeItem(key) } catch {} }
export function clearSession() { remove(SESSION_KEY); remove(TRANSACTION_KEY); window.dispatchEvent(new Event('ohagent-aaas-session')) }
export function readSession(config?: AuthConfig): Session | null {
  try {
    const raw = sessionStorage.getItem(SESSION_KEY)
    if (!raw) return null
    const session = JSON.parse(raw) as Session
    if (typeof session.accessToken !== 'string' || !session.accessToken || !Number.isFinite(session.expiresAt) || session.expiresAt <= Date.now() + 5000 || !session.subject || (config && (session.issuer !== config.issuer || session.clientId !== config.client_id))) { remove(SESSION_KEY); return null }
    return session
  } catch { return null }
}
function secureEndpoint(value: string): URL {
  const url = new URL(value)
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) && url.origin === location.origin
  if ((url.protocol !== 'https:' && !(local && url.protocol === 'http:')) || url.username || url.password || url.hash) throw new ClientError('invalidAuth', 'unsafe_oidc_endpoint')
  return url
}
export async function beginLogin(auth: AuthConfig, returnTo: string): Promise<void> {
  try {
    secureEndpoint(auth.issuer)
    const endpoint = secureEndpoint(auth.authorization_endpoint)
    secureEndpoint(auth.token_endpoint)
    if (!auth.client_id || auth.client_id.length > 1024) throw new ClientError('invalidAuth')
    const transaction: Transaction = { state: random(), nonce: random(), verifier: random(), createdAt: Date.now(), returnTo: safeReturnRoute(returnTo, location.origin), issuer: auth.issuer, clientId: auth.client_id }
    const challenge = base64url(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(transaction.verifier))))
    store(TRANSACTION_KEY, transaction)
    endpoint.searchParams.set('client_id', auth.client_id)
    endpoint.searchParams.set('redirect_uri', redirectUri())
    endpoint.searchParams.set('response_type', 'code')
    endpoint.searchParams.set('scope', 'openid profile')
    endpoint.searchParams.set('state', transaction.state)
    endpoint.searchParams.set('nonce', transaction.nonce)
    endpoint.searchParams.set('code_challenge', challenge)
    endpoint.searchParams.set('code_challenge_method', 'S256')
    location.assign(endpoint.href)
  } catch (error) { if (error instanceof ClientError) throw error; throw new ClientError('invalidAuth') }
}
function decodePart(value: string): Uint8Array {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - value.length % 4) % 4)
  return Uint8Array.from(atob(padded), char => char.charCodeAt(0))
}
async function oidcJson(url: URL, init?: RequestInit) {
  const response = await fetch(url, { ...init, credentials: 'omit', redirect: 'error', cache: 'no-store', signal: AbortSignal.timeout(60_000) })
  if (!response.ok) throw new ClientError('invalidAuth', 'oidc_http_error', response.status)
  return response.json()
}
async function verifyIdentity(idToken: string, auth: AuthConfig, nonce: string): Promise<string> {
  const parts = idToken.split('.')
  if (parts.length !== 3 || idToken.length > 32768) throw new ClientError('invalidAuth', 'invalid_id_token')
  const header = JSON.parse(new TextDecoder().decode(decodePart(parts[0])))
  const claims = JSON.parse(new TextDecoder().decode(decodePart(parts[1])))
  if (!['RS256', 'ES256'].includes(header.alg) || typeof header.kid !== 'string' || header.crit) throw new ClientError('invalidAuth', 'unsupported_id_token')
  const audience = Array.isArray(claims.aud) ? claims.aud : [claims.aud]
  const now = Date.now() / 1000
  if (claims.iss !== auth.issuer || !audience.includes(auth.client_id) || (audience.length > 1 && claims.azp !== auth.client_id) || (claims.azp && claims.azp !== auth.client_id) || claims.nonce !== nonce || typeof claims.sub !== 'string' || !claims.sub || !Number.isFinite(claims.exp) || claims.exp <= now || !Number.isFinite(claims.iat) || claims.iat > now + 60 || (claims.nbf && claims.nbf > now + 60)) throw new ClientError('invalidAuth', 'id_token_claims_rejected')
  const discovery = await oidcJson(secureEndpoint(`${auth.issuer.replace(/\/$/, '')}/.well-known/openid-configuration`))
  if (discovery.issuer !== auth.issuer || discovery.authorization_endpoint !== auth.authorization_endpoint || discovery.token_endpoint !== auth.token_endpoint || typeof discovery.jwks_uri !== 'string') throw new ClientError('invalidAuth', 'discovery_mismatch')
  const jwks = await oidcJson(secureEndpoint(discovery.jwks_uri))
  if (!Array.isArray(jwks.keys)) throw new ClientError('invalidAuth', 'invalid_jwks')
  const matches = jwks.keys.filter((key: JsonWebKey & { kid?: string }) => key.kid === header.kid && (!key.use || key.use === 'sig') && (!key.alg || key.alg === header.alg) && (!key.key_ops || key.key_ops.includes('verify')) && key.kty === (header.alg === 'RS256' ? 'RSA' : 'EC'))
  if (matches.length !== 1) throw new ClientError('invalidAuth', 'signing_key_not_found')
  const algorithm = header.alg === 'RS256' ? { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' } : { name: 'ECDSA', namedCurve: 'P-256', hash: 'SHA-256' }
  const key = await crypto.subtle.importKey('jwk', matches[0], algorithm, false, ['verify'])
  const valid = await crypto.subtle.verify(algorithm, key, new Uint8Array(decodePart(parts[2])), new TextEncoder().encode(`${parts[0]}.${parts[1]}`))
  if (!valid) throw new ClientError('invalidAuth', 'id_token_signature_rejected')
  return claims.sub
}
// Share the callback promise across React StrictMode remounts. Authorization codes are single use.
let callbackPromise: Promise<string> | null = null
export function finishLogin(auth: AuthConfig): Promise<string> {
  if (callbackPromise) return callbackPromise
  callbackPromise = finish(auth)
  return callbackPromise
}
async function finish(auth: AuthConfig): Promise<string> {
  const params = new URLSearchParams(location.search)
  history.replaceState(history.state, '', `${APP_ROOT}/callback`)
  let transaction: Transaction | null = null
  try { transaction = JSON.parse(sessionStorage.getItem(TRANSACTION_KEY) || 'null') } catch {}
  remove(TRANSACTION_KEY)
  try {
    if (!transaction || params.getAll('state').length !== 1 || params.get('state') !== transaction.state || params.has('error') || params.getAll('code').length !== 1 || !params.get('code') || Date.now() - transaction.createdAt > 600_000 || transaction.createdAt > Date.now() + 60_000 || transaction.issuer !== auth.issuer || transaction.clientId !== auth.client_id || !transaction.nonce || !transaction.verifier) throw new ClientError('invalidAuth', 'oidc_state_rejected')
    if (params.has('iss') && params.get('iss') !== auth.issuer) throw new ClientError('invalidAuth', 'issuer_mismatch')
    const tokens = await oidcJson(secureEndpoint(auth.token_endpoint), { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ grant_type: 'authorization_code', code: params.get('code')!, client_id: auth.client_id, redirect_uri: redirectUri(), code_verifier: transaction.verifier }) })
    if (typeof tokens.access_token !== 'string' || !tokens.access_token || tokens.token_type?.toLowerCase() !== 'bearer' || !Number.isFinite(tokens.expires_in) || tokens.expires_in <= 0 || typeof tokens.id_token !== 'string') throw new ClientError('invalidAuth', 'invalid_token_response')
    const subject = await verifyIdentity(tokens.id_token, auth, transaction.nonce)
    // No refresh token, authorization code, ID token or verifier is persisted after exchange.
    store(SESSION_KEY, { accessToken: tokens.access_token, expiresAt: Date.now() + Math.min(tokens.expires_in, 86400) * 1000, issuer: auth.issuer, clientId: auth.client_id, subject } satisfies Session)
    window.dispatchEvent(new Event('ohagent-aaas-session'))
    return safeReturnRoute(transaction.returnTo, location.origin)
  } catch (error) { remove(SESSION_KEY); if (error instanceof ClientError) throw error; throw new ClientError('invalidAuth', 'oidc_verification_failed') }
}
