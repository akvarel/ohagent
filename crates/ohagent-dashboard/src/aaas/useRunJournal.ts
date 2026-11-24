import { useEffect, useRef, useState } from 'react'
import { apiFetch, getRun } from './api'
import { asClientError, ClientError } from './errors'
import type { CopyKey } from './i18n'
import { acceptEvent, acceptSnapshot, isDurableEvent, isFreshDispatch, newJournal, retryDelay, SseDecoder, terminal } from './protocol'
import type { Journal } from './protocol'
import type { DurableEvent, Run } from './types'

export interface DispatchPulse { eventId: string; edgeId?: string; taskId?: string; at: number }
export function useRunJournal(runId: string, replay: boolean, revision: number) {
  const [journal, setJournal] = useState<Journal>(() => newJournal(runId))
  const [transport, setTransport] = useState<CopyKey>('connecting')
  const [error, setError] = useState<ClientError | null>(null)
  const [pulses, setPulses] = useState<DispatchPulse[]>([])
  const applyRef = useRef<(value: Run) => void>(() => {})
  const generation = useRef(0)
  useEffect(() => {
    const own = ++generation.current
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    const alive = () => !controller.signal.aborted && generation.current === own
    let state = newJournal(runId)
    let replayState = newJournal(runId)
    let replayTarget = 0
    let attempt = 0
    const publish = () => { if (alive()) setJournal(state) }
    applyRef.current = value => { if (!alive()) return; state = acceptSnapshot(state, value); publish() }
    setError(null); setPulses([]); setTransport('connecting'); publish()
    const snapshot = async () => {
      const value = await getRun(runId, controller.signal)
      if (!alive()) return
      const next = acceptSnapshot(state, value)
      if (next === state) throw new ClientError('protocol', 'stale_or_invalid_snapshot')
      state = next
      publish()
      return value
    }
    const wait = (ms: number) => new Promise<void>(resolve => { timer = setTimeout(resolve, ms); controller.signal.addEventListener('abort', () => { clearTimeout(timer); resolve() }, { once: true }) })
    const consume = async (event: DurableEvent, connectedAt: number) => {
      const prior = replay ? replayState : state
      const next = acceptEvent(prior, event)
      if (next === prior) {
        if (event.run_id !== runId) throw new ClientError('protocol', 'wrong_run_event')
        if (event.run_seq > prior.cursor + 1) throw new ClientError('protocol', 'journal_gap')
        return
      }
      if (replay) { replayState = next; state = { ...state, events: next.events } }
      else state = next
      if (isFreshDispatch(event, connectedAt, replay) && document.visibilityState === 'visible') {
        const edgeId = typeof event.payload.edge_id === 'string' ? event.payload.edge_id : undefined
        if (alive()) setPulses(previous => [...previous.filter(pulse => Date.now() - pulse.at < 1500), { eventId: event.event_id, edgeId, taskId: event.task_id, at: Date.now() }].slice(-32))
      }
      publish()
    }
    const run = async () => {
      try {
        const initial = await snapshot()
        if (!alive() || !initial) return
        replayTarget = initial.watermark
        if (replay && replayTarget === 0) { setTransport('replayFinished'); return }
        if (!replay && terminal(initial.status)) { setTransport('finished'); return }
        while (alive()) {
          let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
          try {
            const connectedAt = Date.now()
            const cursor = replay ? replayState.cursor : state.cursor
            const response = await apiFetch(`/api/aaas/runs/${encodeURIComponent(runId)}/events?after=${cursor}`, { headers: { Accept: 'text/event-stream' }, signal: controller.signal })
            if (!response.headers.get('content-type')?.includes('text/event-stream') || !response.body) throw new ClientError('protocol', 'invalid_event_stream')
            if (!alive()) return
            setTransport('connected'); setError(null)
            reader = response.body.getReader()
            const decoder = new TextDecoder()
            const sse = new SseDecoder()
            let reachedTarget = false
            while (alive() && !reachedTarget) {
              const chunk = await reader.read()
              if (chunk.done) break
              const frames = sse.push(decoder.decode(chunk.value, { stream: true }))
              let changed = false
              for (const frame of frames) {
                if (frame.event === 'heartbeat') continue
                if (frame.event === 'auth_required') throw new ClientError('unauthorized', 'unauthorized', 401)
                let value: unknown = null
                if (frame.data) { try { value = JSON.parse(frame.data) } catch { throw new ClientError('protocol', 'invalid_event_json') } }
                if (frame.event === 'reset_required' || (value as { type?: string } | null)?.type === 'reset_required') throw new ClientError('protocol', 'reset_required')
                if (!isDurableEvent(value) || (frame.id && frame.id !== String(value.run_seq))) throw new ClientError('protocol', 'invalid_event_envelope')
                if (replay && value.run_seq > replayTarget) { reachedTarget = true; break }
                await consume(value, connectedAt)
                changed = true
                if (replay && replayState.cursor >= replayTarget) { reachedTarget = true; break }
              }
              if (changed && !replay) {
                await snapshot()
                if (state.snapshot && terminal(state.snapshot.status) && state.cursor >= state.snapshot.watermark) { if (alive()) setTransport('finished'); return }
              }
              if (changed) attempt = 0
            }
            if (replay && replayState.cursor >= replayTarget) { if (alive()) setTransport('replayFinished'); return }
            if (!replay) {
              await snapshot()
              if (state.snapshot && terminal(state.snapshot.status) && state.cursor >= state.snapshot.watermark) { if (alive()) setTransport('finished'); return }
            }
            throw new ClientError('network', 'stream_closed')
          } catch (failure) {
            if (!alive()) return
            const issue = asClientError(failure)
            if (issue.status === 401 || issue.status === 403 || issue.status === 404) { setError(issue); return }
            if (issue.code === 'reset_required' || issue.code === 'journal_gap') {
              setTransport('reset')
              if (replay) { setError(new ClientError('protocol', 'replay_history_unavailable')); return }
              await snapshot()
              state = { ...state, cursor: state.snapshot?.watermark ?? state.cursor, events: [], seen: new Set() }
              publish()
            } else { setTransport('reconnecting'); setError(issue) }
            await wait(retryDelay(attempt++, Math.random()))
          } finally { if (reader) await reader.cancel().catch(() => {}) }
        }
      } catch (failure) { if (alive()) setError(asClientError(failure)) }
    }
    void run()
    return () => { controller.abort(); clearTimeout(timer) }
  }, [runId, replay, revision])
  const applySnapshot = (value: Run) => applyRef.current(value)
  return { journal, transport, error, pulses, applySnapshot }
}
