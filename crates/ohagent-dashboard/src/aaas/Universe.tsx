import { useEffect, useId, useRef, useState } from 'react'
import { statusText } from './i18n'
import type { Translate } from './i18n'
import type { Run, Task } from './types'
import type { DispatchPulse } from './useRunJournal'

export function roleName(task: Task, t: Translate): string {
  const role = task.agent_definition_id.split(/[/:.]/).pop() || ''
  return ['coordinator', 'analyst', 'evidence_checker', 'reviewer'].includes(role) ? t(role as 'coordinator' | 'analyst' | 'evidence_checker' | 'reviewer') : task.agent_definition_id
}
function position(task: Task, tasks: Task[]) {
  const role = task.agent_definition_id.split(/[/:.]/).pop() || ''
  const points: Record<string, { x: number; y: number }> = {
    coordinator: { x: 180, y: 260 }, analyst: { x: 480, y: 140 },
    evidence_checker: { x: 480, y: 390 }, reviewer: { x: 780, y: 260 },
  }
  const base = points[role]
  if (base) {
    const row = tasks.filter(item => item.agent_definition_id === task.agent_definition_id).findIndex(item => item.id === task.id)
    return { x: base.x, y: base.y + Math.max(0, row) * 250 }
  }
  return { x: 180 + (tasks.indexOf(task) % 3) * 300, y: 650 + Math.floor(tasks.indexOf(task) / 3) * 250 }
}
function readSelection(id: string): string { try { return sessionStorage.getItem(`ohagent-aaas-selected:${id}`) || '' } catch { return '' } }
export default function Universe({ run, pulses, reduced, t, replay }: { run: Run; pulses: DispatchPulse[]; reduced: boolean; t: Translate; replay: boolean }) {
  const [selectedId, setSelectedId] = useState(() => readSelection(run.id))
  const [query, setQuery] = useState('')
  const [showMap, setShowMap] = useState(false)
  const [view, setView] = useState({ x: 0, y: 0, width: 960, height: 520 })
  const [visiblePulses, setVisiblePulses] = useState<DispatchPulse[]>([])
  const svg = useRef<SVGSVGElement>(null)
  const drag = useRef<{ x: number; y: number; originX: number; originY: number } | null>(null)
  const marker = `arrow-${useId().replace(/:/g, '')}`
  useEffect(() => { try { sessionStorage.setItem(`ohagent-aaas-selected:${run.id}`, selectedId) } catch {} }, [selectedId, run.id])
  useEffect(() => {
    setVisiblePulses(reduced ? [] : pulses.filter(pulse => Date.now() - pulse.at < 1400))
    const timer = setTimeout(() => setVisiblePulses([]), 1450)
    return () => clearTimeout(timer)
  }, [pulses, reduced])
  const selected = run.tasks.find(task => task.id === selectedId)
  const matched = (task: Task) => [roleName(task, t), task.id, task.agent_definition_id, task.agent_instance_id || ''].some(value => value.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
  const filtered = run.tasks.filter(matched)
  const stageHeight = Math.max(440, ...run.tasks.map(task => position(task, run.tasks).y + 100))
  const fit = () => setView({ x: 0, y: 0, width: 960, height: stageHeight })
  const zoom = (factor: number) => setView(previous => { const width = Math.max(320, Math.min(2400, previous.width * factor)); const height = previous.height * width / previous.width; return { x: previous.x + (previous.width - width) / 2, y: previous.y + (previous.height - height) / 2, width, height } })
  const pan = (x: number, y: number) => setView(previous => ({ ...previous, x: Math.max(-2400, Math.min(2400, previous.x + x * previous.width / 10)), y: Math.max(-2400, Math.min(stageHeight + 1000, previous.y + y * previous.height / 10)) }))
  const focus = () => {
    const task = run.tasks.find(item => item.status === 'running') || selected || run.tasks.find(item => item.status === 'queued') || run.tasks[0]
    if (!task) return
    setSelectedId(task.id)
    const point = position(task, run.tasks)
    setView({ x: point.x - 240, y: point.y - 160, width: 480, height: 320 })
  }
  const edgeTask = (id: string) => run.tasks.find(task => task.id === id || task.agent_definition_id === id)
  return <section className="aaas-universe" aria-label={t('map')}>
    <div className="aaas-section-heading"><div><p className="aaas-eyebrow">{t(replay ? 'replay' : 'live')} / {t('snapshot')} {run.watermark}</p><h2>{t('map')}</h2></div><button className="aaas-mobile-toggle" onClick={() => setShowMap(value => !value)} aria-expanded={showMap}>{t(showMap ? 'hideMap' : 'showMap')}</button></div>
    <label className="aaas-search"><span>{t('search')}</span><input type="search" value={query} maxLength={200} onChange={event => setQuery(event.target.value)} /></label>
    <div className="aaas-universe-layout">
      <div className={`aaas-map-panel ${showMap ? 'aaas-map-open' : ''}`}>
        <div className="aaas-map-tools" role="group" aria-label={t('map')}>
          <button onClick={fit}>{t('fit')}</button><button onClick={focus}>{t('focus')}</button>
          <button aria-label={t('zoomIn')} onClick={() => zoom(.8)}>+</button><button aria-label={t('zoomOut')} onClick={() => zoom(1.25)}>−</button>
          <button aria-label={t('panLeft')} onClick={() => pan(-1, 0)}>←</button><button aria-label={t('panRight')} onClick={() => pan(1, 0)}>→</button><button aria-label={t('panUp')} onClick={() => pan(0, -1)}>↑</button><button aria-label={t('panDown')} onClick={() => pan(0, 1)}>↓</button>
        </div>
        <svg ref={svg} className="aaas-map" viewBox={`${view.x} ${view.y} ${view.width} ${view.height}`} role="group" aria-label={t('map')}
          onPointerDown={event => { if ((event.target as Element).closest('[role="button"]')) return; drag.current = { x: event.clientX, y: event.clientY, originX: view.x, originY: view.y }; event.currentTarget.setPointerCapture(event.pointerId) }}
          onPointerMove={event => { if (!drag.current || !svg.current) return; const rect = svg.current.getBoundingClientRect(); const scale = Math.min(rect.width / view.width, rect.height / view.height); const start = drag.current; setView(previous => ({ ...previous, x: Math.max(-2400, Math.min(2400, start.originX - (event.clientX - start.x) / scale)), y: Math.max(-2400, Math.min(stageHeight + 1000, start.originY - (event.clientY - start.y) / scale)) })) }}
          onPointerUp={() => { drag.current = null }} onPointerCancel={() => { drag.current = null }}>
          <defs><marker id={marker} markerWidth="10" markerHeight="8" refX="9" refY="4" orient="auto"><path d="M0,0 L10,4 L0,8" fill="currentColor" /></marker></defs>
          {run.edges.map(edge => {
            const from = edgeTask(edge.from), to = edgeTask(edge.to)
            if (!from || !to) return null
            const a = position(from, run.tasks), b = position(to, run.tasks)
            const path = `M${a.x + 76},${a.y} C${a.x + 140},${a.y} ${b.x - 140},${b.y} ${b.x - 84},${b.y}`
            return <g key={edge.id} className={`aaas-edge aaas-edge-${edge.status}`}><title>{roleName(from, t)} → {roleName(to, t)}: {statusText(edge.status, t)}</title><path d={path} markerEnd={`url(#${marker})`} />
              {!reduced && visiblePulses.filter(pulse => pulse.edgeId === edge.id || (!pulse.edgeId && pulse.taskId === to.id)).map(pulse => <circle key={pulse.eventId} r="7" className="aaas-dispatch-pulse"><animateMotion dur="1.2s" repeatCount="1" path={path} fill="freeze" /></circle>)}
            </g>
          })}
          {run.tasks.map(task => { const p = position(task, run.tasks); return <g key={task.id} transform={`translate(${p.x} ${p.y})`} role="button" tabIndex={0} aria-pressed={selectedId === task.id} aria-label={`${roleName(task, t)}: ${statusText(task.status, t)}. ${t('instance')}: ${task.agent_instance_id || t('notStarted')}`} className={`aaas-node aaas-node-${task.status} ${selectedId === task.id ? 'aaas-selected' : ''} ${!matched(task) ? 'aaas-dim' : ''}`} onClick={() => setSelectedId(task.id)} onKeyDown={event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); setSelectedId(task.id) } }}>
            <circle r="74" className="aaas-node-ring" /><circle r="62" className="aaas-node-body" /><text textAnchor="middle" y="-5">{roleName(task, t).slice(0, 22)}</text><text textAnchor="middle" y="19" className="aaas-node-status">{statusText(task.status, t).slice(0, 26)}</text><text textAnchor="middle" y="104" className="aaas-node-instance">{task.agent_instance_id ? `${t('instance')} · ${task.agent_instance_id.slice(0, 10)}` : t('notStarted')}</text>
          </g> })}
        </svg>
      </div>
      <div className="aaas-stage-panel"><h3>{t('stages')}</h3><ol className="aaas-stages">{filtered.map(task => <li key={task.id}><button aria-pressed={selectedId === task.id} onClick={() => setSelectedId(task.id)}><span className={`aaas-dot aaas-dot-${task.status}`} /><span><strong>{roleName(task, t)}</strong><small>{statusText(task.status, t)}</small></span><span aria-hidden="true">↗</span></button></li>)}</ol>{filtered.length === 0 && <p>{t('noMatch')}</p>}</div>
    </div>
    <aside className="aaas-inspector" aria-live="polite">{selected ? <><h3>{roleName(selected, t)}</h3><dl><div><dt>{t('definition')}</dt><dd>{selected.agent_definition_id}</dd></div><div><dt>{t('instance')}</dt><dd>{selected.agent_instance_id || t('notStarted')}</dd></div><div><dt>{t('attempt')}</dt><dd>{selected.attempt_id}</dd></div><div><dt>{t('status')}</dt><dd>{statusText(selected.status, t)}</dd></div>{selected.error_code && <div><dt>{t('errorCode')}</dt><dd>{/^[a-zA-Z0-9_.-]{1,80}$/.test(selected.error_code) ? selected.error_code : t('error')}</dd></div>}</dl><p className="aaas-muted">{t('noRaw')}</p></> : <p>{t('selectStage')}</p>}</aside>
  </section>
}
