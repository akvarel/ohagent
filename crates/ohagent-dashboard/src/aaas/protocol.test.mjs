import test from 'node:test'
import assert from 'node:assert/strict'
import { APP_ROOT, FEED_LIMIT, acceptEvent, acceptSnapshot, isDurableEvent, isFreshDispatch, newJournal, retryDelay, safeReturnRoute, SseDecoder, terminal } from './protocol.ts'

// Explicit synthetic protocol fixtures. These are never imported by the product UI.
const snapshot = (overrides = {}) => ({ id: 'fixture-run', process_id: 'fixture-process', process_version: 1, title: 'Fixture only', goal: 'Test reducer', status: 'running', version: 1, watermark: 0, created_at: '', updated_at: '', tasks: [], edges: [], ...overrides })
const event = (seq, overrides = {}) => ({ schema_version: 1, event_id: `fixture-event-${seq}`, type: 'edge.dispatched', run_id: 'fixture-run', run_seq: seq, recorded_at: '2026-11-24T21:00:01Z', occurred_at: '2026-11-24T21:00:01Z', payload: { edge_id: 'fixture-edge' }, ...overrides })

test('return route is same-origin, bounded and restricted to the client subtree', () => {
  const fallback = `${APP_ROOT}/processes`
  const origin = 'https://orangehat.test'
  for (const value of [null, '', 'https://evil.test/', '//evil.test/', '/operator', '/agent-as-a-service/app2/runs', '/agent-as-a-service/app/../demo', `${APP_ROOT}/callback`, `${APP_ROOT}/%63allback`, `${APP_ROOT}/%2e%2e/operator`, `${APP_ROOT}/%5cevil`, `${APP_ROOT}//evil`, `${APP_ROOT}/bad%`, `${APP_ROOT}/bad\npath`, `${APP_ROOT}/${'a'.repeat(2048)}`]) assert.equal(safeReturnRoute(value, origin), fallback, String(value))
  assert.equal(safeReturnRoute(`${APP_ROOT}/runs/run-1?mode=replay#journal`, origin), `${APP_ROOT}/runs/run-1?mode=replay#journal`)
})
test('snapshot journal rejects wrong run, old versions, stale cursor and malformed arrays', () => {
  let state = acceptSnapshot(newJournal('fixture-run'), snapshot({ watermark: 4, version: 2 }))
  assert.equal(state.cursor, 4)
  for (const change of [{ id: 'other' }, { watermark: 3 }, { version: 1 }, { watermark: 2.5 }, { tasks: null }, { edges: {} }]) assert.equal(acceptSnapshot(state, snapshot({ watermark: 4, version: 2, ...change })), state)
  state = acceptEvent(state, event(5))
  assert.equal(acceptSnapshot(state, snapshot({ watermark: 4, version: 3 })), state)
  assert.equal(acceptSnapshot(state, snapshot({ watermark: 5, version: 3 })).snapshot.version, 3)
})
test('newer snapshots never skip unread journal events', () => {
  let state = acceptSnapshot(newJournal('fixture-run'), snapshot())
  state = acceptEvent(state, event(1))
  state = acceptSnapshot(state, snapshot({ watermark: 4, version: 4, status: 'completed' }))
  assert.equal(state.cursor, 1)
  for (let seq = 2; seq <= 4; seq++) state = acceptEvent(state, event(seq))
  assert.deepEqual(state.events.map(item => item.run_seq), [1, 2, 3, 4])
  assert.equal(state.cursor, state.snapshot.watermark)
})

test('durable journal deduplicates IDs and sequence numbers and rejects gaps and cross-run delivery', () => {
  const first = acceptEvent(newJournal('fixture-run'), event(1))
  assert.equal(first.cursor, 1)
  for (const candidate of [event(1), event(2, { event_id: 'fixture-event-1' }), event(0), event(3), event(2, { run_id: 'foreign-run' }), event(2, { schema_version: 2 })]) assert.equal(acceptEvent(first, candidate), first)
  const second = acceptEvent(first, event(2))
  assert.equal(second.cursor, 2)
  assert.equal(second.events.length, 2)
})
test('event feed and dedup cache are bounded without permitting old sequence replay', () => {
  let state = newJournal('fixture-run')
  for (let i = 1; i <= FEED_LIMIT * 3; i++) state = acceptEvent(state, event(i))
  assert.equal(state.events.length, FEED_LIMIT)
  assert.equal(state.seen.size, FEED_LIMIT)
  assert.equal(state.events[0].run_seq, FEED_LIMIT * 2 + 1)
  assert.equal(acceptEvent(state, event(1)), state)
})
test('event schema fails closed on non-durable or malformed payloads', () => {
  for (const value of [null, {}, event(1, { run_seq: NaN }), event(1, { payload: null }), event(1, { payload: [] }), event(1, { event_id: '' })]) assert.equal(isDurableEvent(value), false)
  assert.equal(isDurableEvent(event(1)), true)
})
test('pulses are eligible only for fresh dispatches, never replay or reconnect backlog', () => {
  const connected = Date.parse('2026-11-24T21:00:00Z')
  assert.equal(isFreshDispatch(event(1), connected, false), true)
  assert.equal(isFreshDispatch(event(1), connected, true), false)
  assert.equal(isFreshDispatch(event(1), connected + 2000, false), false)
  assert.equal(isFreshDispatch(event(1, { occurred_at: '2026-11-24T20:59:59Z' }), connected, false), false)
  assert.equal(isFreshDispatch(event(1, { type: 'task.completed' }), connected, false), false)
  assert.equal(isFreshDispatch(event(1, { type: 'edge_dispatched' }), connected, false), true)
})
test('SSE decoder handles every split boundary, CRLF, comments and multiline data', () => {
  const source = ': heartbeat\r\n\r\nid: 7\r\nevent: durable\r\ndata: {"first":1,\r\ndata: "second":2}\r\n\r\n'
  const expected = [{ event: 'durable', id: '7', data: '{"first":1,\n"second":2}' }]
  for (let i = 0; i <= source.length; i++) {
    const decoder = new SseDecoder()
    assert.deepEqual([...decoder.push(source.slice(0, i)), ...decoder.push(source.slice(i))], expected, `split ${i}`)
  }
})
test('SSE decoder recognizes reset_required without data and bounds frames', () => {
  const decoder = new SseDecoder()
  assert.deepEqual(decoder.push('event: reset_required\n\n'), [{ event: 'reset_required', id: '', data: '' }])
  assert.throws(() => decoder.push('x'.repeat(1_048_577)), /stream_too_large/)
})
test('cancellation_requested is not a terminal cancellation and unknown usage remains absent', () => {
  assert.equal(terminal('cancellation_requested'), false)
  assert.equal(terminal('cancelled'), true)
  assert.equal(terminal('interrupted'), true)
  const run = snapshot({ status: 'cancellation_requested' })
  assert.equal(run.usage, undefined)
  assert.equal(acceptSnapshot(newJournal('fixture-run'), run).snapshot.status, 'cancellation_requested')
})
test('backoff is exponential and capped with deterministic jitter', () => {
  assert.equal(retryDelay(0), 1000)
  assert.equal(retryDelay(1), 2000)
  assert.equal(retryDelay(50), 30000)
  assert.equal(retryDelay(0, 0), 800)
  assert.equal(retryDelay(0, 1), 1200.0000000000002)
})
