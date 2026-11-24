import type { DurableEvent, Run } from './types.ts'

export const APP_ROOT = '/agent-as-a-service/app'
export const FEED_LIMIT = 160
export function safeReturnRoute(value: string | null, origin: string): string {
  const fallback = `${APP_ROOT}/processes`
  if (!value || value.length > 2048 || /[\\\u0000-\u001f]/.test(value) || !value.startsWith('/') || value.startsWith('//')) return fallback
  try {
    const url = new URL(value, origin)
    const decoded = decodeURIComponent(url.pathname)
    if (url.origin !== origin || !decoded.startsWith(`${APP_ROOT}/`) || decoded.includes('..') || decoded.includes('\\') || decoded.includes('//') || decoded === `${APP_ROOT}/callback`) return fallback
    return `${url.pathname}${url.search}${url.hash}`
  } catch { return fallback }
}
export interface Journal { runId: string; cursor: number; snapshot: Run | null; events: DurableEvent[]; seen: Set<string> }
export function newJournal(runId: string): Journal { return { runId, cursor: 0, snapshot: null, events: [], seen: new Set() } }
export function acceptSnapshot(state: Journal, snapshot: Run): Journal {
  if (!snapshot || snapshot.id !== state.runId || !Number.isSafeInteger(snapshot.watermark) || snapshot.watermark < state.cursor || !Number.isSafeInteger(snapshot.version) || !Array.isArray(snapshot.tasks) || !Array.isArray(snapshot.edges)) return state
  if (state.snapshot && (snapshot.version < state.snapshot.version || snapshot.watermark < state.snapshot.watermark)) return state
  // Only the initial baseline may skip history. Later snapshots must not acknowledge unread SSE events.
  return { ...state, snapshot, cursor: state.snapshot ? state.cursor : snapshot.watermark }
}
export function isDurableEvent(value: unknown): value is DurableEvent {
  if (!value || typeof value !== 'object') return false
  const event = value as DurableEvent
  return event.schema_version === 1 && typeof event.event_id === 'string' && !!event.event_id && typeof event.run_id === 'string' && Number.isSafeInteger(event.run_seq) && event.run_seq > 0 && typeof event.type === 'string' && typeof event.recorded_at === 'string' && typeof event.occurred_at === 'string' && !!event.payload && typeof event.payload === 'object' && !Array.isArray(event.payload)
}
export function acceptEvent(state: Journal, event: DurableEvent): Journal {
  if (!isDurableEvent(event) || event.run_id !== state.runId || state.seen.has(event.event_id) || event.run_seq <= state.cursor) return state
  // A hole requires a fresh authoritative snapshot. Never advance across missing events.
  if (event.run_seq !== state.cursor + 1) return state
  const events = [...state.events, event].slice(-FEED_LIMIT)
  return { ...state, cursor: event.run_seq, events, seen: new Set(events.map(item => item.event_id)) }
}
export function isFreshDispatch(event: DurableEvent, connectedAt: number, replay: boolean): boolean {
  return !replay && /(?:^|[._])(?:edge_)?dispatched$/.test(event.type) && Date.parse(event.recorded_at) >= connectedAt && Date.parse(event.occurred_at) >= connectedAt
}
export interface SseMessage { event: string; id: string; data: string }
export class SseDecoder {
  private buffer = ''
  push(chunk: string): SseMessage[] {
    this.buffer += chunk
    if (this.buffer.length > 1_048_576) throw new Error('stream_too_large')
    const messages: SseMessage[] = []
    // Normalize complete CRLF pairs, retaining a trailing CR across chunk boundaries.
    this.buffer = this.buffer.replace(/\r\n/g, '\n')
    let boundary: number
    while ((boundary = this.buffer.indexOf('\n\n')) >= 0) {
      const frame = this.buffer.slice(0, boundary)
      this.buffer = this.buffer.slice(boundary + 2)
      let event = 'message', id = ''
      const data: string[] = []
      for (const line of frame.split('\n')) {
        if (line.startsWith(':')) continue
        const colon = line.indexOf(':')
        const field = colon < 0 ? line : line.slice(0, colon)
        const value = colon < 0 ? '' : line.slice(colon + 1).replace(/^ /, '')
        if (field === 'data') data.push(value)
        if (field === 'event') event = value
        if (field === 'id' && !value.includes('\0')) id = value
      }
      if (data.length || event === 'reset_required') messages.push({ event, id, data: data.join('\n') })
    }
    return messages
  }
}
export function retryDelay(attempt: number, jitter = 0.5): number { return Math.min(30_000, 1000 * 2 ** Math.min(attempt, 5)) * (0.8 + 0.4 * jitter) }
export const terminal = (status: string) => ['completed', 'failed', 'cancelled', 'interrupted'].includes(status)
