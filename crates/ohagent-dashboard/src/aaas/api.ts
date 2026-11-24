import { clearSession, readSession } from './auth'
import { ClientError, responseError } from './errors'
import type { Process, ProcessInput, PublicConfig, Run } from './types'

export async function apiFetch(path: string, init: RequestInit = {}, authenticated = true): Promise<Response> {
  if (!path.startsWith('/api/aaas/') || path.includes('://')) throw new ClientError('protocol')
  const headers = new Headers(init.headers)
  if (authenticated) {
    const session = readSession()
    if (!session) { clearSession(); throw new ClientError('unauthorized') }
    headers.set('Authorization', `Bearer ${session.accessToken}`)
  }
  if (init.body) headers.set('Content-Type', 'application/json')
  const signal = init.signal ? AbortSignal.any([init.signal, AbortSignal.timeout(60_000)]) : AbortSignal.timeout(60_000)
  const response = await fetch(path, { ...init, headers, credentials: 'omit', cache: 'no-store', redirect: 'error', signal })
  if (!response.ok) { if (response.status === 401) clearSession(); throw await responseError(response) }
  return response
}
export async function json<T>(path: string, init?: RequestInit, authenticated = true): Promise<T> {
  const response = await apiFetch(path, init, authenticated)
  try { return await response.json() as T } catch { throw new ClientError('protocol', 'invalid_json') }
}
export const getConfig = (signal?: AbortSignal) => json<PublicConfig>('/api/aaas/config', { signal }, false)
export const listProcesses = (signal?: AbortSignal) => json<{ processes: Process[] }>('/api/aaas/processes', { signal })
export const listRuns = (signal?: AbortSignal) => json<{ runs: Run[] }>('/api/aaas/runs', { signal })
export const getRun = (id: string, signal?: AbortSignal) => json<Run>(`/api/aaas/runs/${encodeURIComponent(id)}`, { signal })
export const saveProcess = (input: ProcessInput, id?: string) => json<Process>(`/api/aaas/processes${id ? `/${encodeURIComponent(id)}` : ''}`, { method: id ? 'PUT' : 'POST', body: JSON.stringify(input) })
export const startRun = (process: Process, key: string) => json<Run>('/api/aaas/runs', { method: 'POST', headers: { 'Idempotency-Key': key }, body: JSON.stringify({ process_id: process.id, process_version: process.version }) })
export const cancelRun = (run: Run) => json<Run>(`/api/aaas/runs/${encodeURIComponent(run.id)}/cancel`, { method: 'POST', body: JSON.stringify({ version: run.version }) })
export function startRequestKey(process: Process): string {
  const session = readSession()
  if (!session) throw new ClientError('unauthorized')
  const storageKey = `ohagent-aaas-start:${JSON.stringify([session.issuer, session.subject, process.id, process.version])}`
  try {
    const existing = sessionStorage.getItem(storageKey)
    if (existing) return existing
    const key = crypto.randomUUID()
    sessionStorage.setItem(storageKey, key)
    return key
  } catch { throw new ClientError('storage') }
}
export function finishStartRequest(process: Process) {
  const session = readSession()
  if (session) try { sessionStorage.removeItem(`ohagent-aaas-start:${JSON.stringify([session.issuer, session.subject, process.id, process.version])}`) } catch {}
}
export async function downloadArtifact(run: Run): Promise<void> {
  const response = await apiFetch(`/api/aaas/runs/${encodeURIComponent(run.id)}/artifact`)
  if (!response.headers.get('content-type')?.toLowerCase().startsWith('text/markdown')) throw new ClientError('protocol', 'artifact_content_type')
  const blob = await response.blob()
  if (blob.size > 10 * 1024 * 1024) throw new ClientError('protocol', 'artifact_too_large')
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `brief-${run.id.replace(/[^a-zA-Z0-9_-]/g, '_').slice(0, 80)}.md`
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
